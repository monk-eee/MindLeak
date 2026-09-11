use std::{sync::Mutex, time::Duration};

use ackplane_protocol::v1::{
    self,
    node_sync_service_server::{NodeSyncService, NodeSyncServiceServer},
};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::{transport::Server, Request, Response, Status, Streaming};

use super::*;
use crate::{ClientError, NodeSyncConnection};

struct RefusingSigner(SigningError);

impl ClaimSigner for RefusingSigner {
    fn signing_key_id(&self) -> &str {
        "key-unavailable"
    }

    fn node_id(&self) -> &str {
        "node-test"
    }

    fn sign(&self, _bytes: &[u8]) -> Result<Vec<u8>, SigningError> {
        Err(self.0)
    }
}

// Provider loss must return a local refusal, never panic or fabricate a signature.
#[test]
fn authentication_returns_signing_failure_without_fabricating_a_signature() {
    for error in [
        SigningError::Unavailable,
        SigningError::IdentityMismatch,
        SigningError::Refused,
    ] {
        let signer = RefusingSigner(error);
        let results = [
            authenticate(
                &signer,
                "tenant-test",
                "repository-test",
                "task-test",
                "owner-test",
                &ClaimOperation::Release,
            ),
            authenticate_lifecycle_purge(
                &signer,
                "tenant-test",
                "repository-test",
                &LifecyclePurgeOperation::Confirm {
                    request_id: "purge-test",
                },
            ),
            authenticate_recovery_execution(
                &signer,
                "tenant-test",
                "repository-test",
                &RecoveryExecutionOperation::Confirm {
                    request_id: "recovery-test",
                },
            ),
        ];
        for result in results {
            assert_eq!(result, Err(error));
        }
    }
}

type FollowingFrame = Result<Option<v1::NodeFrame>, Status>;

struct ChallengeService {
    observed: Mutex<Option<oneshot::Sender<FollowingFrame>>>,
}

#[tonic::async_trait]
impl NodeSyncService for ChallengeService {
    type SynchronizeStream = ReceiverStream<Result<v1::AckplaneFrame, Status>>;

    async fn synchronize(
        &self,
        request: Request<Streaming<v1::NodeFrame>>,
    ) -> Result<Response<Self::SynchronizeStream>, Status> {
        let observed = self.observed.lock().unwrap().take().unwrap();
        let mut incoming = request.into_inner();
        let (sender, outgoing) = mpsc::channel(1);
        tokio::spawn(async move {
            let hello = incoming.message().await.unwrap().unwrap();
            assert!(matches!(hello.frame, Some(v1::node_frame::Frame::Hello(_))));
            sender
                .send(Ok(v1::AckplaneFrame {
                    frame: Some(v1::ackplane_frame::Frame::ConnectionChallenge(
                        v1::ConnectionChallenge { nonce: vec![7; 32] },
                    )),
                }))
                .await
                .unwrap();
            let _ = observed.send(incoming.message().await);
        });
        Ok(Response::new(ReceiverStream::new(outgoing)))
    }
}

#[tokio::test]
async fn signing_failure_closes_the_handshake_without_sending_a_challenge_response() {
    tokio::time::timeout(Duration::from_secs(10), async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (observed, observation) = oneshot::channel();
        let (shutdown, stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            Server::builder()
                .add_service(NodeSyncServiceServer::new(ChallengeService {
                    observed: Mutex::new(Some(observed)),
                }))
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        let result = NodeSyncConnection::open(
            &endpoint,
            &RefusingSigner(SigningError::Unavailable),
            "tenant-test",
            "repository-test",
            vec!["synchronize".to_string()],
            0,
        )
        .await;
        assert!(matches!(
            result,
            Err(ClientError::Signing(SigningError::Unavailable))
        ));
        assert!(
            !matches!(observation.await.unwrap(), Ok(Some(_))),
            "signer refusal must not send any frame after Hello"
        );
        shutdown.send(()).unwrap();
        server.await.unwrap();
    })
    .await
    .expect("the refused connection must close promptly");
}

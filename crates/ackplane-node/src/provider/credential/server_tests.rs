use std::time::Duration;

use ackplane_protocol::{
    connection_challenge_auth::{connection_challenge_bytes, ConnectionChallengeBinding},
    v1::{
        self, node_enrollment_service_client::NodeEnrollmentServiceClient,
        node_enrollment_service_server::NodeEnrollmentServiceServer,
        node_sync_service_client::NodeSyncServiceClient,
        node_sync_service_server::NodeSyncServiceServer,
    },
};
use ackplane_server::{
    db_pool::{build_pool, TEST_POOL_MAX_SIZE},
    enrollment_service::NodeEnrollmentService,
    enrollment_store::{EnrollmentApproval, EnrollmentStore},
    ledger::LedgerStore,
    service::NodeSyncService,
};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::{transport::Server, Request};

use super::{tests::credential, CredentialCandidate, CredentialProvider};
use crate::{NodeSigner, SigningBinding};

#[tokio::test]
async fn a_recovered_candidate_replays_activation_and_authenticates_with_the_assigned_key() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    tokio::time::timeout(Duration::from_secs(20), async {
        let pool = build_pool(&database_url, TEST_POOL_MAX_SIZE).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let enrollment = NodeEnrollmentService::new(EnrollmentStore::connect(&pool).await.unwrap());
        let sync = NodeSyncService::new(
            LedgerStore::connect(&pool).await.unwrap(),
            v1::FlowControl {
                max_in_flight_batches: 4,
                max_batch_bytes: 1_048_576,
            },
        );
        let (shutdown, stopped) = oneshot::channel();
        let server = tokio::spawn(async move {
            Server::builder()
                .add_service(NodeEnrollmentServiceServer::new(enrollment))
                .add_service(NodeSyncServiceServer::new(sync))
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let tenant_id = format!(
            "provider-{}-{}",
            std::process::id(),
            directory.path().file_name().unwrap().to_string_lossy()
        );
        let request_id = format!("request-{tenant_id}");
        let entry = credential();
        let mut candidate = CredentialCandidate::provision_with(
            &tenant_id,
            "repo-test",
            "node-test",
            directory.path(),
            |_| Ok(entry.clone()),
        )
        .unwrap();
        let original = candidate.identity();
        let mut client = NodeEnrollmentServiceClient::connect(endpoint.clone())
            .await
            .unwrap();
        client
            .submit_enrollment_request(Request::new(v1::EnrollmentRequest {
                request_id: request_id.clone(),
                tenant_id: tenant_id.clone(),
                repository_id: "repo-test".to_string(),
                proposed_node_id: original.node_id.clone(),
                display_name: "Persistent provider enrollment test".to_string(),
                public_key_fingerprint: original.fingerprint.clone(),
                requested_capabilities: vec!["synchronize".to_string()],
                created_at: "2026-01-01T00:00:00Z".to_string(),
                expires_at: "2030-01-01T00:00:00Z".to_string(),
                public_key: original.public_key.to_vec(),
            }))
            .await
            .unwrap();
        EnrollmentStore::connect(&pool)
            .await
            .unwrap()
            .approve(&EnrollmentApproval {
                request_id: request_id.clone(),
                tenant_id: tenant_id.clone(),
                repository_id: "repo-test".to_string(),
                public_key_fingerprint: original.fingerprint.clone(),
                approved_capabilities: vec!["synchronize".to_string()],
                approved_by: "provider-test-administrator".to_string(),
            })
            .await
            .unwrap();
        let challenge = client
            .get_activation_challenge(Request::new(v1::EnrollmentChallengeRequest {
                request_id,
                tenant_id: tenant_id.clone(),
                repository_id: "repo-test".to_string(),
                proposed_node_id: original.node_id.clone(),
                public_key_fingerprint: original.fingerprint.clone(),
            }))
            .await
            .unwrap()
            .into_inner();
        let proof = candidate.activation_proof(&challenge).unwrap();
        let committed = client
            .activate_enrollment(Request::new(proof.clone()))
            .await
            .unwrap()
            .into_inner();
        drop(candidate);

        let recovered =
            CredentialCandidate::recover_with(&tenant_id, "repo-test", directory.path(), |_| {
                Ok(entry.clone())
            })
            .unwrap();
        let retry = recovered.retry_activation_proof().unwrap();
        assert_eq!(retry, proof);
        let mut altered = retry.clone();
        altered.nonce[0] ^= 1;
        assert!(client
            .activate_enrollment(Request::new(altered))
            .await
            .is_err());
        let replayed = client
            .activate_enrollment(Request::new(retry))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(replayed.signing_key_id, committed.signing_key_id);
        assert_eq!(
            replayed.enrolment_receipt_id,
            committed.enrolment_receipt_id
        );
        let provider = recovered.accept_activation(&replayed).unwrap();
        assert_eq!(provider.identity().public_key, original.public_key);
        drop(provider);

        for _restart in 0..2 {
            let provider =
                CredentialProvider::recover_with(&tenant_id, "repo-test", directory.path(), |_| {
                    Ok(entry.clone())
                })
                .unwrap();
            let identity = provider.identity();
            assert_eq!(identity.public_key, original.public_key);
            assert_eq!(identity.signing_key_id, committed.signing_key_id);
            let mut sync = NodeSyncServiceClient::connect(endpoint.clone())
                .await
                .unwrap();
            let (outgoing, requests) = mpsc::channel::<v1::NodeFrame>(4);
            let mut incoming = sync
                .synchronize(Request::new(ReceiverStream::new(requests)))
                .await
                .unwrap()
                .into_inner();
            outgoing
                .send(v1::NodeFrame {
                    frame: Some(v1::node_frame::Frame::Hello(v1::Hello {
                        tenant_id: tenant_id.clone(),
                        repository_id: "repo-test".to_string(),
                        producer_id: identity.node_id.clone(),
                        last_accepted_position: 0,
                        capabilities: vec!["synchronize".to_string()],
                        signing_key_id: identity.signing_key_id.clone(),
                    })),
                })
                .await
                .unwrap();
            let nonce = match incoming.message().await.unwrap().unwrap().frame {
                Some(v1::ackplane_frame::Frame::ConnectionChallenge(challenge)) => challenge.nonce,
                other => panic!("expected connection challenge, got {other:?}"),
            };
            let bytes = connection_challenge_bytes(&ConnectionChallengeBinding {
                nonce: &nonce,
                tenant_id: &tenant_id,
                repository_id: "repo-test",
                producer_id: &identity.node_id,
                signing_key_id: &identity.signing_key_id,
            });
            let signature = provider
                .sign(
                    "node_sync.connection",
                    &SigningBinding {
                        tenant_id: tenant_id.clone(),
                        repository_id: "repo-test".to_string(),
                        node_id: identity.node_id,
                        key_id: identity.signing_key_id,
                    },
                    &bytes,
                )
                .unwrap();
            outgoing
                .send(v1::NodeFrame {
                    frame: Some(v1::node_frame::Frame::ChallengeResponse(
                        v1::ChallengeResponse {
                            signature: signature.as_bytes().to_vec(),
                        },
                    )),
                })
                .await
                .unwrap();
            assert!(matches!(
                incoming.message().await.unwrap().unwrap().frame,
                Some(v1::ackplane_frame::Frame::HelloAccepted(_))
            ));
            assert!(matches!(
                incoming.message().await.unwrap().unwrap().frame,
                Some(v1::ackplane_frame::Frame::FlowControl(_))
            ));
            drop(outgoing);
            assert!(incoming.message().await.unwrap().is_none());
        }
        let connection = pool.get().await.unwrap();
        for table in ["signing_keys", "enrollment_receipts"] {
            let count: i64 = connection
                .query_one(
                    &format!("SELECT COUNT(*) FROM {table} WHERE tenant_id = $1"),
                    &[&tenant_id],
                )
                .await
                .unwrap()
                .get(0);
            assert_eq!(count, 1, "recovery must not create a second {table} record");
        }
        drop(client);
        shutdown.send(()).unwrap();
        server.await.unwrap();
    })
    .await
    .expect("the real enrollment and reconnect workflow must finish without hanging");
}

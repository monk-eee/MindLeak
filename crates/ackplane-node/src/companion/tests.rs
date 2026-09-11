use super::*;
use crate::{NodeProcessLock, SoftwareProvider};
use prost::Message;

// Collapsing permission denial into an outage made callers retry an authoritative refusal.
#[test]
fn permission_refusal_remains_nonretryable_without_echoing_remote_payloads() {
    let reply = refused(ClientError::Rejected(tonic::Status::permission_denied(
        "private payload",
    )));
    assert!(
        matches!(reply, NodeReply::Refused { reason, retryable: false, ref diagnostic } if reason == v1::RejectionReason::Unauthorized as i32 && !diagnostic.contains("private payload"))
    );
}

#[test]
fn rejected_frames_keep_the_outbox_refusal_category() {
    let reply = refused(ClientError::FrameRefused {
        reason: v1::RejectionReason::Malformed,
        retryable: false,
        diagnostic: "out of scope".into(),
    });
    let NodeReply::Frame(bytes) = reply else {
        panic!("frame refusal became a connection retry");
    };
    let frame = v1::AckplaneFrame::decode(bytes.as_slice()).unwrap();
    assert!(matches!(
        frame.frame,
        Some(v1::ackplane_frame::Frame::Rejection(v1::Rejection {
            retryable: false,
            ..
        }))
    ));
}
use ackplane_client::companion::NodeClient;

#[tokio::test]
async fn a_correctly_scoped_client_receives_an_identity_response() {
    let directory = tempfile::tempdir().unwrap();
    let _owner = NodeProcessLock::acquire(directory.path()).unwrap();
    let signer = Arc::new(SoftwareProvider::generate("tenant", "repository", "node"));
    let service = NodeService::new(
        "tenant".into(),
        "repository".into(),
        "http://127.0.0.1:1".into(),
        signer,
    )
    .unwrap();
    let listener = service.bind(directory.path()).unwrap();
    let (_stop, stopped) = watch::channel(false);
    let server = tokio::spawn(async move {
        service
            .handle(listener.accept().await.unwrap(), stopped)
            .await
    });
    let identity = NodeClient::new(
        directory.path().into(),
        "tenant".into(),
        "repository".into(),
    )
    .identity()
    .await
    .unwrap();
    assert_eq!(identity.node_id, "node");
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn a_request_declaring_a_different_repository_id_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let _owner = NodeProcessLock::acquire(directory.path()).unwrap();
    let signer = Arc::new(SoftwareProvider::generate("tenant", "repository", "node"));
    let service = NodeService::new(
        "tenant".into(),
        "repository".into(),
        "http://127.0.0.1:1".into(),
        signer,
    )
    .unwrap();
    let listener = service.bind(directory.path()).unwrap();
    let (_stop, stopped) = watch::channel(false);
    let server = tokio::spawn(async move {
        service
            .handle(listener.accept().await.unwrap(), stopped)
            .await
    });
    assert!(
        NodeClient::new(directory.path().into(), "tenant".into(), "another".into())
            .identity()
            .await
            .is_err()
    );
    server.await.unwrap().unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn a_live_endpoint_is_never_unlinked_and_stale_socket_recovery_preserves_metadata() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let _owner = NodeProcessLock::acquire(directory.path()).unwrap();
    let listener = endpoint::bind(directory.path()).unwrap();
    assert_eq!(
        std::fs::metadata(directory.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(directory.path().join("ackplane-node.sock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(
        matches!(endpoint::bind(directory.path()), Err(error) if error.kind() == io::ErrorKind::AddrInUse)
    );
    drop(listener);
    let socket =
        std::os::unix::net::UnixListener::bind(directory.path().join("ackplane-node.sock"))
            .unwrap();
    drop(socket);
    std::fs::write(
        directory.path().join("enrolment.json"),
        "public test metadata",
    )
    .unwrap();
    let _recovered = endpoint::bind(directory.path()).unwrap();
    assert_eq!(
        std::fs::read_to_string(directory.path().join("enrolment.json")).unwrap(),
        "public test metadata"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_state_or_endpoint_is_refused_without_removing_the_target() {
    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("target");
    std::fs::create_dir(&target).unwrap();
    let alias = directory.path().join("alias");
    std::os::unix::fs::symlink(&target, &alias).unwrap();
    assert!(endpoint::bind(&alias).is_err());
    let endpoint_path = directory.path().join("ackplane-node.sock");
    std::os::unix::fs::symlink(&target, &endpoint_path).unwrap();
    assert!(endpoint::bind(directory.path()).is_err());
    assert_eq!(std::fs::read_link(endpoint_path).unwrap(), target);
    assert!(target.is_dir());
}

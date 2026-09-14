use std::time::Duration;

use ackplane_client::companion::{
    endpoint_name,
    wire::{read_message, write_message, NodeReply, NodeRequest, Operation},
};
use ackplane_protocol::v1;
use interprocess::local_socket::{tokio::prelude::*, ListenerOptions};
use prost::Message;

use super::{compiled_federation_readiness, FederationReadiness};

fn probe(reply: NodeReply) -> FederationReadiness {
    let directory = tempfile::tempdir().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let listener = runtime.block_on(async {
        ListenerOptions::new()
            .name(endpoint_name(directory.path()).unwrap())
            .create_tokio()
            .unwrap()
    });
    let server = std::thread::spawn(move || {
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(2), async {
                let mut stream = listener.accept().await.unwrap();
                let request: NodeRequest = read_message(&mut stream).await.unwrap();
                assert_eq!(request.tenant_id, "probe-tenant");
                assert_eq!(request.repository_id, "probe-repository");
                assert!(matches!(request.body, Operation::EnrollmentStatus));
                assert!(request.expected_endpoint.is_none());
                write_message(&mut stream, &reply).await.unwrap();
            })
            .await
            .is_ok()
        })
    });
    let readiness = compiled_federation_readiness(&|name| match name {
        "MINDLEAK_ACKPLANE_STATE_DIR" => Some(directory.path().to_string_lossy().into_owned()),
        "MINDLEAK_ACKPLANE_TENANT_ID" => Some("probe-tenant".into()),
        "MINDLEAK_ACKPLANE_REPOSITORY_ID" => Some("probe-repository".into()),
        "MINDLEAK_ACKPLANE_ENDPOINT" => Some("http://127.0.0.1:1".into()),
        "MINDLEAK_ACKPLANE_TLS_CA_PATH" => Some("unused-client-ca.pem".into()),
        _ => None,
    });
    assert!(
        server.join().unwrap(),
        "readiness did not consult the companion"
    );
    readiness
}

// Local planes rejected an active companion because a separate raw gRPC endpoint was required.
#[test]
fn an_active_companion_enables_federation_without_a_direct_grpc_connection() {
    let reply = NodeReply::Payload(
        v1::EnrollmentStatusResult {
            state: v1::EnrollmentState::Active as i32,
            verified: true,
        }
        .encode_to_vec(),
    );
    assert_eq!(probe(reply), FederationReadiness::Ready);
}

#[test]
fn unverified_inactive_and_unknown_enrollment_states_never_enable_federation() {
    for (state, verified) in [
        (v1::EnrollmentState::Active as i32, false),
        (v1::EnrollmentState::Approved as i32, true),
        (v1::EnrollmentState::Activating as i32, true),
        (i32::MAX, true),
    ] {
        let reply =
            NodeReply::Payload(v1::EnrollmentStatusResult { state, verified }.encode_to_vec());
        assert_eq!(probe(reply), FederationReadiness::NotEnrolled);
    }
}

#[test]
fn companion_refusals_distinguish_remote_outage_from_revoked_enrollment() {
    for (retryable, expected) in [
        (true, FederationReadiness::ArbiterUnreachable),
        (false, FederationReadiness::NotEnrolled),
    ] {
        let reply = NodeReply::Refused {
            reason: v1::RejectionReason::NodeRevoked as i32,
            retryable,
            diagnostic: "refused by fixture".into(),
        };
        assert_eq!(probe(reply), expected);
    }
}

#[test]
fn malformed_companion_status_is_unavailable_not_ready() {
    assert_eq!(
        probe(NodeReply::Payload(vec![0xff])),
        FederationReadiness::CompanionUnavailable,
    );
}

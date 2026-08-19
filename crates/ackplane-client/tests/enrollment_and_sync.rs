//! Proof that the full node lifecycle -- enrollment, activation, and one
//! Synchronize round trip -- works end to end over real gRPC, closing
//! `gaps.d/no-client-has-ever-enrolled-with-or-synced-to-ackplane.md`.
//!
//! Skipped unless `ACKPLANE_TEST_DATABASE_URL` names the gated test
//! PostgreSQL database. Starts real `NodeEnrollmentService` and
//! `NodeSyncService` instances on an ephemeral loopback port, mirroring
//! `tests/arbitration.rs`'s existing pattern.
//!
//! Run against the compose topology with, e.g.:
//! ```text
//! docker compose up -d postgres
//! ACKPLANE_TEST_DATABASE_URL=postgres://ackplane:ackplane-development-only-not-for-production@127.0.0.1:5432/ackplane cargo test -p ackplane-client --test enrollment_and_sync
//! ```

use ackplane_protocol::v1::{
    self, node_enrollment_service_client::NodeEnrollmentServiceClient,
    node_enrollment_service_server::NodeEnrollmentServiceServer,
    node_sync_service_client::NodeSyncServiceClient,
    node_sync_service_server::NodeSyncServiceServer,
};
use ackplane_server::{
    enrollment::{
        activation_challenge_bytes, connection_challenge_bytes, public_key_fingerprint,
        ConnectionChallengeBinding,
    },
    enrollment_service::NodeEnrollmentService,
    enrollment_store::{EnrollmentApproval, EnrollmentStore},
    envelope_signature::envelope_signing_bytes,
    ledger::LedgerStore,
    service::NodeSyncService,
};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
use tonic::{transport::Server, Code, Request};

fn unique_id(label: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("ackplane-client-{label}-{nanos}")
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

#[tokio::test]
async fn a_fresh_node_enrolls_activates_and_synchronizes_one_real_event() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };

    let enrollment_store = EnrollmentStore::connect(&database_url)
        .await
        .expect("the gated test database should accept enrollment migrations");
    let ledger = LedgerStore::connect(&database_url)
        .await
        .expect("the gated test database should accept ledger migrations");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("the test service should bind loopback");
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(NodeEnrollmentServiceServer::new(
                NodeEnrollmentService::new(enrollment_store),
            ))
            .add_service(NodeSyncServiceServer::new(NodeSyncService::new(
                ledger,
                v1::FlowControl {
                    max_in_flight_batches: 16,
                    max_batch_bytes: 1_048_576,
                },
            )))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("the test service should run");
    });
    let endpoint = format!("http://{address}");

    let tenant_id = unique_id("tenant");
    let repository_id = unique_id("repository");
    let node_id = unique_id("node");
    let request_id = unique_id("request");

    let signing_key = SigningKey::from_bytes(&[91_u8; 32]);
    let public_key = signing_key.verifying_key().to_bytes().to_vec();
    let fingerprint = public_key_fingerprint(&public_key);

    let mut enrollment_client = NodeEnrollmentServiceClient::connect(endpoint.clone())
        .await
        .expect("the in-process enrollment service should accept the connection");

    let submitted = enrollment_client
        .submit_enrollment_request(Request::new(v1::EnrollmentRequest {
            request_id: request_id.clone(),
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            proposed_node_id: node_id.clone(),
            display_name: "test node".to_string(),
            public_key_fingerprint: fingerprint.clone(),
            requested_capabilities: vec!["synchronize".to_string()],
            created_at: now_rfc3339(),
            expires_at: "2030-01-01T00:00:00Z".to_string(),
            public_key: public_key.clone(),
        }))
        .await
        .expect("submit_enrollment_request should round-trip over the wire")
        .into_inner();
    assert_eq!(submitted.state(), v1::EnrollmentState::Pending);

    // Before approval, activation is refused with FailedPrecondition -- the
    // real arbitration this gap's fix must prove, not merely a socket answer.
    let refused = enrollment_client
        .get_activation_challenge(Request::new(v1::EnrollmentChallengeRequest {
            request_id: request_id.clone(),
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            proposed_node_id: node_id.clone(),
            public_key_fingerprint: fingerprint.clone(),
        }))
        .await
        .expect_err("an unapproved request must be refused, not silently issued a challenge");
    assert_eq!(refused.code(), Code::FailedPrecondition);

    let mut store = EnrollmentStore::connect(&database_url)
        .await
        .expect("the gated test database should accept a second enrollment connection");
    store
        .approve(&EnrollmentApproval {
            request_id: request_id.clone(),
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            public_key_fingerprint: fingerprint.clone(),
            approved_capabilities: vec!["synchronize".to_string()],
            approved_by: "test-administrator".to_string(),
        })
        .await
        .expect("approval should succeed for a pending request");

    let challenge = enrollment_client
        .get_activation_challenge(Request::new(v1::EnrollmentChallengeRequest {
            request_id: request_id.clone(),
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            proposed_node_id: node_id.clone(),
            public_key_fingerprint: fingerprint.clone(),
        }))
        .await
        .expect("an approved request should be issued a real activation challenge")
        .into_inner();
    let proof_bytes = activation_challenge_bytes(
        &challenge.nonce,
        &request_id,
        &tenant_id,
        &repository_id,
        &node_id,
        &fingerprint,
    );
    let activation = enrollment_client
        .activate_enrollment(Request::new(v1::EnrollmentActivationProof {
            request_id: request_id.clone(),
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            proposed_node_id: node_id.clone(),
            public_key_fingerprint: fingerprint.clone(),
            nonce: challenge.nonce.clone(),
            signature: signing_key.sign(&proof_bytes).to_bytes().to_vec(),
        }))
        .await
        .expect("activation with a genuine proof of possession should succeed")
        .into_inner();
    assert_eq!(activation.state(), v1::EnrollmentState::Activating);
    assert!(!activation.enrolment_receipt_id.is_empty());

    let (db_client, connection) = tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
        .await
        .expect("the gated test database should accept a signing-key read connection");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let signing_key_id: String = db_client
        .query_one(
            "SELECT signing_key_id FROM signing_keys WHERE public_key_fingerprint = $1 \
             ORDER BY activated_at DESC LIMIT 1",
            &[&fingerprint],
        )
        .await
        .expect("activation should have registered a signing key")
        .get(0);

    let mut sync_client = NodeSyncServiceClient::connect(endpoint)
        .await
        .expect("the in-process sync service should accept the connection");
    let (tx, rx) = mpsc::channel::<v1::NodeFrame>(4);
    let mut inbound = sync_client
        .synchronize(Request::new(ReceiverStream::new(rx)))
        .await
        .expect("synchronize should open a real bidirectional stream")
        .into_inner();

    tx.send(v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::Hello(v1::Hello {
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            producer_id: node_id.clone(),
            last_accepted_position: 0,
            capabilities: vec!["synchronize".to_string()],
            signing_key_id: signing_key_id.clone(),
        })),
    })
    .await
    .unwrap();

    let nonce = match inbound
        .message()
        .await
        .unwrap()
        .expect("the stream should send a ConnectionChallenge")
        .frame
    {
        Some(v1::ackplane_frame::Frame::ConnectionChallenge(challenge)) => challenge.nonce,
        other => panic!("expected ConnectionChallenge, got {other:?}"),
    };
    tx.send(v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::ChallengeResponse(
            v1::ChallengeResponse {
                signature: signing_key
                    .sign(&connection_challenge_bytes(&ConnectionChallengeBinding {
                        nonce: &nonce,
                        tenant_id: &tenant_id,
                        repository_id: &repository_id,
                        producer_id: &node_id,
                        signing_key_id: &signing_key_id,
                    }))
                    .to_bytes()
                    .to_vec(),
            },
        )),
    })
    .await
    .unwrap();

    assert!(matches!(
        inbound.message().await.unwrap().unwrap().frame,
        Some(v1::ackplane_frame::Frame::HelloAccepted(_))
    ));
    assert!(matches!(
        inbound.message().await.unwrap().unwrap().frame,
        Some(v1::ackplane_frame::Frame::FlowControl(_))
    ));

    let payload = b"integration test: repository activity".to_vec();
    let payload_digest = Sha256::digest(&payload).to_vec();
    let mut wire = v1::EventEnvelope {
        tenant_id: tenant_id.clone(),
        repository_id: repository_id.clone(),
        producer_id: node_id.clone(),
        producer_sequence: 1,
        payload,
        payload_digest,
        schema_version: "1".to_string(),
        occurred_at: now_rfc3339(),
        payload_type: "test.repository_activity".to_string(),
        previous_envelope_digest: Vec::new(),
        signing_key_id: signing_key_id.clone(),
        signature: Vec::new(),
        provenance: v1::ProvenanceClass::EnrolledNode as i32,
    };
    wire.signature = signing_key
        .sign(&envelope_signing_bytes(&wire))
        .to_bytes()
        .to_vec();
    tx.send(v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::EventBatch(v1::EventBatch {
            events: vec![wire],
        })),
    })
    .await
    .unwrap();

    let receipt = match inbound
        .message()
        .await
        .unwrap()
        .expect("the stream should send a BatchReceipt")
        .frame
    {
        Some(v1::ackplane_frame::Frame::BatchReceipt(receipt)) => receipt,
        other => panic!("expected BatchReceipt, got {other:?}"),
    };
    assert_eq!(receipt.receipts.len(), 1);
    assert_eq!(receipt.receipts[0].position, 1);

    drop(tx);
    let _ = shutdown_tx.send(());
    server.abort();
}

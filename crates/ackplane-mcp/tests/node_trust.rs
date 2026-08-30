//! Proves ADR-0137 clause 6: Ackplane's `NodeSync` protocol tolerates a
//! second connection signed by the same enrolled node key while a first one
//! (e.g. an already-connected `ackplane-supervisor`) is still open, which the
//! ADR's own drafting left an open implementation question rather than an
//! assumption. Skipped unless `ACKPLANE_TEST_DATABASE_URL` names the gated
//! test PostgreSQL database, mirroring
//! `ackplane-client/tests/enrollment_and_sync.rs`'s existing pattern.

use ackplane_client::{NodeSyncConnection, SeedSigner};
use ackplane_protocol::v1::{
    self, node_enrollment_service_client::NodeEnrollmentServiceClient,
    node_enrollment_service_server::NodeEnrollmentServiceServer,
    node_sync_service_server::NodeSyncServiceServer,
};
use ackplane_server::{
    enrollment::public_key_fingerprint,
    enrollment_service::NodeEnrollmentService,
    enrollment_store::{EnrollmentApproval, EnrollmentStore},
    ledger::LedgerStore,
    service::NodeSyncService,
};
use ed25519_dalek::{Signer, SigningKey};
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{transport::Server, Request};

fn unique_id(label: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("ackplane-mcp-node-trust-{label}-{nanos}")
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap()
}

// This proves the mechanism `ackplane-mcp`'s `node_trust::establish` (ADR-
// 0137 clause 1) relies on: a second `NodeSyncConnection::open` call, signed
// by the same node key as an already-open connection, using library calls
// only.
#[tokio::test]
async fn a_second_connection_signed_by_the_same_node_key_is_tolerated_alongside_an_already_open_one(
) {
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

    let seed = [113_u8; 32];
    let signing_key = SigningKey::from_bytes(&seed);
    let public_key = signing_key.verifying_key().to_bytes().to_vec();
    let fingerprint = public_key_fingerprint(&public_key);

    let mut enrollment_client = NodeEnrollmentServiceClient::connect(endpoint.clone())
        .await
        .expect("the in-process enrollment service should accept the connection");
    enrollment_client
        .submit_enrollment_request(Request::new(v1::EnrollmentRequest {
            request_id: request_id.clone(),
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            proposed_node_id: node_id.clone(),
            display_name: "concurrent-connection test node".to_string(),
            public_key_fingerprint: fingerprint.clone(),
            requested_capabilities: vec!["synchronize".to_string(), "mcp-front-door".to_string()],
            created_at: now_rfc3339(),
            expires_at: "2030-01-01T00:00:00Z".to_string(),
            public_key: public_key.clone(),
        }))
        .await
        .expect("submit_enrollment_request should round-trip over the wire");

    let mut store = EnrollmentStore::connect(&database_url)
        .await
        .expect("the gated test database should accept a second enrollment connection");
    store
        .approve(&EnrollmentApproval {
            request_id: request_id.clone(),
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            public_key_fingerprint: fingerprint.clone(),
            approved_capabilities: vec!["synchronize".to_string(), "mcp-front-door".to_string()],
            approved_by: "test-administrator".to_string(),
        })
        .await
        .expect("approval should succeed for a pending request");

    use ackplane_server::enrollment::activation_challenge_bytes;
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
    let signing_key_id = activation.signing_key_id.clone();
    assert!(!signing_key_id.is_empty());

    let signer = SeedSigner::new(signing_key_id.clone(), node_id.clone(), &seed);

    // The already-connected supervisor this node key is also used for.
    let _supervisor_connection = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        NodeSyncConnection::open(
            &endpoint,
            &signer,
            &tenant_id,
            &repository_id,
            vec!["synchronize".to_string()],
            0,
        ),
    )
    .await
    .expect("the first connection does not hang")
    .expect("the first (simulated supervisor) connection authenticates");

    // `ackplane-mcp`'s own `node_trust::establish` (ADR-0137 clause 1) is a
    // thin wrapper around exactly this same `NodeSyncConnection::open` call,
    // with the same node identity, capability name, and starting position --
    // proving this second call succeeds while `_supervisor_connection` is
    // still open proves the mechanism `establish` relies on. `establish`
    // itself was additionally verified by hand against a real compiled
    // `ackplane-mcp` process run alongside this exact harness.
    let second_connection = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        NodeSyncConnection::open(
            &endpoint,
            &signer,
            &tenant_id,
            &repository_id,
            vec!["mcp-front-door".to_string()],
            0,
        ),
    )
    .await
    .expect("the second connection does not hang");

    // `second_connection`'s value is dropped as part of this `map` (its
    // closure takes ownership and returns `()`), matching `_supervisor_
    // connection` below: `serve_with_incoming_shutdown`'s graceful shutdown
    // waits for in-flight streams to close, and this test's own two
    // connections would otherwise block it forever.
    let outcome = second_connection.map(|_connection| ());
    drop(_supervisor_connection);

    let _ = shutdown_tx.send(());
    let _ = server.await;

    if let Err(error) = outcome {
        panic!(
            "a second connection signed by the same node key must not be refused while the \
             first is still open (ADR-0137 clause 6): {error}"
        );
    }
}

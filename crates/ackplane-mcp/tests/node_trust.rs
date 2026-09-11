//! Proves ADR-0137 clause 6: Ackplane's `NodeSync` protocol tolerates a
//! second connection signed by the same enrolled node key while a first one
//! (e.g. an already-connected `ackplane-supervisor`) is still open, which the
//! ADR's own drafting left an open implementation question rather than an
//! assumption. Skipped unless `ACKPLANE_TEST_DATABASE_URL` names the gated
//! test PostgreSQL database, mirroring
//! `ackplane-client/tests/enrollment_and_sync.rs`'s existing pattern.

use ackplane_client::companion::NodeClient;
#[path = "../../ackplane-node/tests/support/companion.rs"]
mod companion;
use ackplane_protocol::enrollment::public_key_fingerprint;
use ackplane_protocol::v1::{
    self, node_enrollment_service_client::NodeEnrollmentServiceClient,
    node_enrollment_service_server::NodeEnrollmentServiceServer,
    node_sync_service_server::NodeSyncServiceServer,
};
use ackplane_server::{
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

    let pool = ackplane_server::db_pool::build_pool(
        &database_url,
        ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
    )
    .expect("the test pool builds from a valid database url");
    let enrollment_store = EnrollmentStore::connect(&pool)
        .await
        .expect("the gated test database should accept enrollment migrations");
    let ledger = LedgerStore::connect(&pool)
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

    let store = EnrollmentStore::connect(&pool)
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

    use ackplane_protocol::enrollment::activation_challenge_bytes;
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

    let directory = tempfile::tempdir().unwrap();
    let companion = companion::TestCompanion::start(
        &endpoint,
        ackplane_node::SigningBinding {
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            node_id: node_id.clone(),
            key_id: signing_key_id,
        },
        &seed,
        directory.path(),
    )
    .await;
    let node = NodeClient::new(
        directory.path().into(),
        tenant_id.clone(),
        repository_id.clone(),
    );

    // The already-connected supervisor this node key is also used for.
    let _supervisor_connection =
        tokio::time::timeout(std::time::Duration::from_secs(15), node.open_sync(0, None))
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
    let second_connection =
        tokio::time::timeout(std::time::Duration::from_secs(15), node.open_sync(0, None))
            .await
            .expect("the second connection does not hang");

    let path = directory.path().to_path_buf();
    let responses = tokio::task::spawn_blocking(move || {
        use std::{io::Write, process::{Command, Stdio}};
        let mut child = Command::new(env!("CARGO_BIN_EXE_ackplane-mcp"))
            .env("ACKPLANE_MCP_ENDPOINT", endpoint)
            .env("MINDLEAK_ACKPLANE_STATE_DIR", path)
            .env("MINDLEAK_ACKPLANE_TENANT_ID", tenant_id)
            .env("MINDLEAK_ACKPLANE_REPOSITORY_ID", repository_id)
            .env_remove("MINDLEAK_ACKPLANE_NODE_ID")
            .env_remove("MINDLEAK_ACKPLANE_SIGNING_KEY_ID")
            .env_remove("MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED")
            .env_remove("MINDLEAK_ACKPLANE_KEY_PATH")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap();
        let mut input = child.stdin.take().unwrap();
        for (index, name) in ["open_session", "check_enrollment_status"].iter().enumerate() {
            let arguments = if index == 0 { serde_json::json!({"session_id":"0123456789abcdef0123456789abcdef"}) } else { serde_json::json!({}) };
            writeln!(input, "{}", serde_json::json!({"jsonrpc":"2.0","id":index,"method":"tools/call","params":{"name":name,"arguments":arguments}})).unwrap();
        }
        drop(input);
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().lines().map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()).collect::<Vec<_>>()
    }).await.unwrap();
    assert_eq!(responses.len(), 2);
    for response in &responses {
        assert_eq!(response["result"]["isError"], false, "{response}");
    }
    let session: serde_json::Value = serde_json::from_str(
        responses[0]["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(session["agent_id"]
        .as_str()
        .unwrap()
        .starts_with("session:v1:"));
    let status: serde_json::Value = serde_json::from_str(
        responses[1]["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(status["verified"], true);
    assert_eq!(status["node_id"], node_id);

    // `second_connection`'s value is dropped as part of this `map` (its
    // closure takes ownership and returns `()`), matching `_supervisor_
    // connection` below: `serve_with_incoming_shutdown`'s graceful shutdown
    // waits for in-flight streams to close, and this test's own two
    // connections would otherwise block it forever.
    let outcome = second_connection.map(|_connection| ());
    drop(_supervisor_connection);
    drop(companion);

    let _ = shutdown_tx.send(());
    let _ = server.await;

    if let Err(error) = outcome {
        panic!(
            "a second connection signed by the same node key must not be refused while the \
             first is still open (ADR-0137 clause 6): {error}"
        );
    }
}

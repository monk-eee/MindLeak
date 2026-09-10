use std::{path::Path, process::Output, time::Duration};

use ackplane_protocol::v1::{
    node_enrollment_service_server::NodeEnrollmentServiceServer,
    node_sync_service_server::NodeSyncServiceServer, FlowControl,
};
use ackplane_server::{
    db_pool::{build_pool, PgPool, TEST_POOL_MAX_SIZE},
    enrollment_service::NodeEnrollmentService,
    enrollment_store::{EnrollmentApproval, EnrollmentStore},
    ledger::LedgerStore,
    service::NodeSyncService,
};
use serde_json::Value;
use tokio::{process::Command, sync::oneshot};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

async fn run_cli(directory: &Path, args: &[&str]) -> Output {
    tokio::time::timeout(
        Duration::from_secs(10),
        Command::new(env!("CARGO_BIN_EXE_register-me"))
            .current_dir(directory)
            .env_remove("MINDLEAK_ACKPLANE_KEY_PATH")
            .env_remove("MINDLEAK_ACKPLANE_TLS_CA_PATH")
            .kill_on_drop(true)
            .args(args)
            .output(),
    )
    .await
    .expect("the enrollment CLI must not hang")
    .expect("the enrollment CLI must start")
}

async fn start_server(
    pool: &PgPool,
    with_sync: bool,
) -> (String, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let enrollment = EnrollmentStore::connect(pool).await.unwrap();
    let sync = if with_sync {
        Some(NodeSyncServiceServer::new(NodeSyncService::new(
            LedgerStore::connect(pool).await.unwrap(),
            FlowControl {
                max_in_flight_batches: 4,
                max_batch_bytes: 1_048_576,
            },
        )))
    } else {
        None
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        Server::builder()
            .add_service(NodeEnrollmentServiceServer::new(
                NodeEnrollmentService::new(enrollment),
            ))
            .add_optional_service(sync)
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    (endpoint, shutdown_tx, server)
}

// Activation used to discard its key ID and receipt before a failed sync, leaving no restart state.
#[tokio::test]
async fn activation_survives_a_failed_sync_and_a_cli_restart_without_replacing_identity() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let pool = build_pool(&database_url, TEST_POOL_MAX_SIZE).unwrap();
    let (endpoint, shutdown_tx, server) = start_server(&pool, false).await;
    let directory = tempfile::tempdir().unwrap();
    let tenant = format!(
        "cli-{}-{}",
        std::process::id(),
        directory.path().file_name().unwrap().to_string_lossy()
    );
    let request_args = [
        "request",
        "--repo",
        "repo-test",
        "--node",
        "node-test",
        "--tenant-id",
        &tenant,
        "--grpc-endpoint",
        &endpoint,
    ];
    let requested = run_cli(directory.path(), &request_args).await;
    assert!(
        requested.status.success(),
        "{}",
        String::from_utf8_lossy(&requested.stderr)
    );

    let key_path = directory.path().join(ackplane_client::DEFAULT_KEY_PATH);
    let state_path = key_path.with_extension("key.enrollment.json");
    let original_key = std::fs::read(&key_path).unwrap();
    let pending: Value = serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
    let request_id = pending["request_id"].as_str().unwrap();
    let store = EnrollmentStore::connect(&pool).await.unwrap();
    store
        .approve(&EnrollmentApproval {
            request_id: request_id.to_string(),
            tenant_id: tenant.clone(),
            repository_id: "repo-test".to_string(),
            public_key_fingerprint: pending["public_key_fingerprint"]
                .as_str()
                .unwrap()
                .to_string(),
            approved_capabilities: vec!["synchronize".to_string()],
            approved_by: "cli-test-admin".to_string(),
        })
        .await
        .unwrap();

    let activated = run_cli(directory.path(), &["activate", "--request-id", request_id]).await;
    assert!(
        !activated.status.success(),
        "this fixture intentionally has no NodeSync service"
    );
    assert!(String::from_utf8_lossy(&activated.stderr).contains("could not open NodeSync"));
    let saved_bytes = std::fs::read(&state_path).unwrap();
    let saved: Value = serde_json::from_slice(&saved_bytes).unwrap();

    shutdown_tx.send(()).unwrap();
    server.await.unwrap();

    assert!(
        !saved["activation"]["signing_key_id"]
            .as_str()
            .unwrap_or("")
            .is_empty(),
        "the assigned signing key ID must survive a failed sync"
    );
    assert!(
        !saved["activation"]["enrolment_receipt_id"]
            .as_str()
            .unwrap_or("")
            .is_empty(),
        "the activation receipt must survive a failed sync"
    );
    assert_eq!(saved["request_id"], pending["request_id"]);
    assert_eq!(
        saved["public_key_fingerprint"],
        pending["public_key_fingerprint"]
    );
    assert!(
        std::fs::read(&key_path).unwrap() == original_key,
        "activation must preserve the key"
    );

    let restarted = run_cli(
        directory.path(),
        &["activate", "--request-id", request_id, "--skip-sync"],
    )
    .await;
    assert!(
        restarted.status.success(),
        "{}",
        String::from_utf8_lossy(&restarted.stderr)
    );
    assert!(String::from_utf8_lossy(&restarted.stdout).contains("recorded activation"));
    assert_eq!(std::fs::read(&state_path).unwrap(), saved_bytes);

    let repeated = run_cli(directory.path(), &request_args).await;
    assert!(
        !repeated.status.success(),
        "a new request must not erase existing enrollment"
    );
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("saved enrollment"));
    assert_eq!(std::fs::read(&state_path).unwrap(), saved_bytes);
    assert!(std::fs::read(&key_path).unwrap() == original_key);

    let (endpoint, shutdown_tx, server) = start_server(&pool, true).await;
    for _attempt in 0..2 {
        let connected = run_cli(
            directory.path(),
            &[
                "activate",
                "--request-id",
                request_id,
                "--grpc-endpoint",
                &endpoint,
            ],
        )
        .await;
        assert!(
            connected.status.success(),
            "{}",
            String::from_utf8_lossy(&connected.stderr)
        );
        assert!(String::from_utf8_lossy(&connected.stdout).contains("authenticated:"));
        assert_eq!(std::fs::read(&state_path).unwrap(), saved_bytes);
        assert!(std::fs::read(&key_path).unwrap() == original_key);
    }
    shutdown_tx.send(()).unwrap();
    server.await.unwrap();
}

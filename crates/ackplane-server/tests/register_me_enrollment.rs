use std::{
    path::Path,
    process::Output,
    sync::{Arc, Mutex},
    time::Duration,
};

use ackplane_protocol::v1::{
    self, node_enrollment_service_server::NodeEnrollmentService as EnrollmentRpc,
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
use tonic::{transport::Server, Request, Response, Status};

struct EnrollmentWithLostResponse {
    inner: NodeEnrollmentService,
    dropped: Option<Arc<Mutex<Option<v1::EnrollmentActivationResult>>>>,
}

#[tonic::async_trait]
impl EnrollmentRpc for EnrollmentWithLostResponse {
    async fn submit_enrollment_request(
        &self,
        request: Request<v1::EnrollmentRequest>,
    ) -> Result<Response<v1::EnrollmentRequestStatus>, Status> {
        self.inner.submit_enrollment_request(request).await
    }

    async fn get_activation_challenge(
        &self,
        request: Request<v1::EnrollmentChallengeRequest>,
    ) -> Result<Response<v1::EnrollmentChallenge>, Status> {
        self.inner.get_activation_challenge(request).await
    }

    async fn activate_enrollment(
        &self,
        request: Request<v1::EnrollmentActivationProof>,
    ) -> Result<Response<v1::EnrollmentActivationResult>, Status> {
        let response = self.inner.activate_enrollment(request).await?;
        if let Some(dropped) = &self.dropped {
            let mut original = dropped.lock().unwrap();
            if original.is_none() {
                *original = Some(response.get_ref().clone());
                return Err(Status::unavailable("activation response lost after commit"));
            }
        }
        Ok(response)
    }

    async fn rotate_node_key(
        &self,
        request: Request<v1::KeyRotationRequest>,
    ) -> Result<Response<v1::KeyRotationResult>, Status> {
        self.inner.rotate_node_key(request).await
    }

    async fn check_enrollment_status(
        &self,
        request: Request<v1::EnrollmentStatusRequest>,
    ) -> Result<Response<v1::EnrollmentStatusResult>, Status> {
        self.inner.check_enrollment_status(request).await
    }
}

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
    dropped: Option<Arc<Mutex<Option<v1::EnrollmentActivationResult>>>>,
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
                EnrollmentWithLostResponse {
                    inner: NodeEnrollmentService::new(enrollment),
                    dropped,
                },
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
    assert_activation_recovery(None).await;
}

// Losing an accepted response stranded the node; replay must recover the original receipt and key.
#[tokio::test]
async fn a_lost_activation_response_is_recovered_after_a_cli_restart() {
    assert_activation_recovery(Some(Arc::new(Mutex::new(None)))).await;
}

async fn assert_activation_recovery(
    dropped: Option<Arc<Mutex<Option<v1::EnrollmentActivationResult>>>>,
) {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let pool = build_pool(&database_url, TEST_POOL_MAX_SIZE).unwrap();
    let (endpoint, shutdown_tx, server) = start_server(&pool, false, dropped.clone()).await;
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
    let activated = if dropped.is_some() {
        assert!(!activated.status.success());
        assert!(String::from_utf8_lossy(&activated.stderr)
            .contains("activation response lost after commit"));
        let interrupted_bytes = std::fs::read(&state_path).unwrap();
        let interrupted: Value = serde_json::from_slice(&interrupted_bytes).unwrap();
        assert!(interrupted["activation"].is_null());
        assert_eq!(
            interrupted["activation_nonce"].as_array().map(Vec::len),
            Some(32),
            "the original challenge must be saved before submitting the activation proof"
        );
        assert!(std::fs::read(&key_path).unwrap() == original_key);

        let mut corrupted = interrupted.clone();
        let first_byte = corrupted["activation_nonce"][0].as_u64().unwrap();
        corrupted["activation_nonce"][0] = serde_json::json!(first_byte ^ 1);
        let corrupted_bytes = serde_json::to_vec(&corrupted).unwrap();
        std::fs::write(&state_path, &corrupted_bytes).unwrap();
        let refused = run_cli(
            directory.path(),
            &["activate", "--request-id", request_id, "--skip-sync"],
        )
        .await;
        assert!(
            !refused.status.success(),
            "a different nonce must not recover a receipt"
        );
        assert!(String::from_utf8_lossy(&refused.stderr).contains("activate_enrollment failed"));
        assert_eq!(std::fs::read(&state_path).unwrap(), corrupted_bytes);
        std::fs::write(&state_path, &interrupted_bytes).unwrap();

        run_cli(directory.path(), &["activate", "--request-id", request_id]).await
    } else {
        activated
    };
    assert!(
        !activated.status.success(),
        "this fixture intentionally has no NodeSync service"
    );
    assert!(String::from_utf8_lossy(&activated.stderr).contains("could not open NodeSync"));
    let saved_bytes = std::fs::read(&state_path).unwrap();
    let saved: Value = serde_json::from_slice(&saved_bytes).unwrap();

    if let Some(dropped) = &dropped {
        let original = dropped.lock().unwrap();
        let original = original
            .as_ref()
            .expect("the server committed one activation");
        assert_eq!(
            saved["activation"]["signing_key_id"],
            original.signing_key_id
        );
        assert_eq!(
            saved["activation"]["enrolment_receipt_id"],
            original.enrolment_receipt_id
        );
        assert!(
            saved["activation_nonce"].is_null(),
            "completed activation needs no retry nonce"
        );
    }

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

    let (endpoint, shutdown_tx, server) = start_server(&pool, true, None).await;
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
    let connection = pool.get().await.unwrap();
    let counts = connection
        .query_one(
            "SELECT (SELECT count(*) FROM enrollment_receipts WHERE tenant_id = $1), \
         (SELECT count(*) FROM signing_keys WHERE tenant_id = $1)",
            &[&tenant],
        )
        .await
        .unwrap();
    assert_eq!(
        counts.get::<_, i64>(0),
        1,
        "replay must not create another receipt"
    );
    assert_eq!(
        counts.get::<_, i64>(1),
        1,
        "replay must not provision a replacement key"
    );
    shutdown_tx.send(()).unwrap();
    server.await.unwrap();
}

use std::sync::{Arc, Mutex};

use ackplane_protocol::v1;
use ackplane_server::{
    db_pool::{build_pool, TEST_POOL_MAX_SIZE},
    enrollment_store::{EnrollmentApproval, EnrollmentStore},
};
use serde_json::Value;

#[path = "register_me_enrollment/companion_tests.rs"]
mod companion_tests;
#[path = "register_me_enrollment/request_tests.rs"]
mod request_tests;
#[path = "register_me_enrollment/support.rs"]
mod support;
use support::{run_cli, start_server, TestIdentity};

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
    let (endpoint, shutdown_tx, server) = start_server(&pool, false, dropped.clone(), None).await;
    let directory = TestIdentity::new();
    let tenant = format!(
        "cli-{}-{}",
        std::process::id(),
        directory.path().file_name().unwrap().to_string_lossy()
    );
    let request_args = [
        "request",
        "--provider",
        "credential-facility-software",
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

    let state_path = directory.path().join("enrolment.json");
    let request_path = directory.path().join("enrollment-request.json");
    let requested_bytes = std::fs::read(&request_path).unwrap();
    let pending: Value = serde_json::from_slice(&requested_bytes).unwrap();
    let original: Value = serde_json::from_slice(&std::fs::read(&state_path).unwrap()).unwrap();
    let original_key = original["public_key"].clone();
    let original_handle = original["provider_handle"].clone();
    assert!(!directory
        .path()
        .join(ackplane_client::DEFAULT_KEY_PATH)
        .exists());
    let repeated = run_cli(directory.path(), &request_args).await;
    assert!(
        repeated.status.success(),
        "{}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert_eq!(std::fs::read(&request_path).unwrap(), requested_bytes);
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
            interrupted["challenge"]["nonce"].as_array().map(Vec::len),
            Some(32),
            "the original challenge must be saved before submitting the activation proof"
        );
        assert_eq!(interrupted["public_key"], original_key);
        assert_eq!(interrupted["provider_handle"], original_handle);

        let mut corrupted = interrupted.clone();
        corrupted["challenge"]["request_id"] = serde_json::json!("another-request");
        let corrupted_bytes = serde_json::to_vec(&corrupted).unwrap();
        std::fs::write(&state_path, &corrupted_bytes).unwrap();
        let refused = run_cli(
            directory.path(),
            &["activate", "--request-id", request_id, "--skip-sync"],
        )
        .await;
        assert!(
            !refused.status.success(),
            "a different request binding must not recover a receipt"
        );
        assert!(String::from_utf8_lossy(&refused.stderr).contains("does not match"));
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
            saved["challenge"].is_null(),
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
    assert_eq!(saved["activation"]["request_id"], pending["request_id"]);
    assert_eq!(saved["fingerprint"], pending["public_key_fingerprint"]);
    assert!(
        saved["public_key"] == original_key,
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
    assert!(String::from_utf8_lossy(&repeated.stderr).contains("already activated"));
    assert_eq!(std::fs::read(&state_path).unwrap(), saved_bytes);
    assert_eq!(saved["provider_handle"], original_handle);
    assert_eq!(std::fs::read(&request_path).unwrap(), requested_bytes);

    let (endpoint, shutdown_tx, server) = start_server(&pool, true, None, None).await;
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
        assert_eq!(std::fs::read(&request_path).unwrap(), requested_bytes);
        assert!(!directory
            .path()
            .join(ackplane_client::DEFAULT_KEY_PATH)
            .exists());
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
    drop(connection);
    companion_tests::exercise(&directory, &endpoint, &tenant, &pool, dropped.is_some()).await;
    shutdown_tx.send(()).unwrap();
    server.await.unwrap();
}

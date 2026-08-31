//! HTTP-level coverage for enrolled-key authenticated Recovery-execution
//! preview/confirmation (ADR-0145 decision 4-5).

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ackplane_bridge::administration::{administration_routes, AdministrationApiState};
use ackplane_client::auth::{authenticate_recovery_execution, ClaimSigner, SeedSigner};
use ackplane_protocol::purge_confirmation_auth::RecoveryExecutionOperation;
use ackplane_server::{
    administration_store::{
        AdministrationOperation, AdministrationScope, AdministrationStore, NewRecoveryRehearsal,
        PolicyAdoptionRequest,
    },
    claim_store::ClaimStore,
    enrollment::{activation_challenge_bytes, public_key_fingerprint},
    enrollment_store::{
        ActivationChallengeRequest, EnrollmentActivation, EnrollmentApproval, EnrollmentStore,
        EnrollmentSubmission,
    },
    fleet::FleetStore,
    snapshot_provider::SnapshotProviderConfig,
};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

const CONFIRMATION_WINDOW_SECONDS: u64 = 900;

fn unique_suffix() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock should be after the Unix epoch")
        .as_nanos();
    let mut random_bytes = [0_u8; 8];
    getrandom::getrandom(&mut random_bytes).expect("the OS random source should be available");
    let random_suffix: String = random_bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{timestamp}-{random_suffix}")
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

async fn enroll_signer(
    database_url: &str,
    tenant_id: &str,
    repository_id: &str,
    suffix: &str,
    label: &str,
    seed: [u8; 32],
) -> SeedSigner {
    let signing_key = SigningKey::from_bytes(&seed);
    let public_key = signing_key.verifying_key().to_bytes();
    let node_id = format!("administration-recovery-{label}-node-{suffix}");
    let signing_key_id = format!("administration-recovery-{label}-signing-key-{suffix}");
    let submission = EnrollmentSubmission {
        request_id: format!("administration-recovery-{label}-request-{suffix}"),
        tenant_id: tenant_id.to_string(),
        repository_id: repository_id.to_string(),
        proposed_node_id: node_id.clone(),
        display_name: format!("Administration recovery integration {label}"),
        public_key: public_key.to_vec(),
        public_key_fingerprint: public_key_fingerprint(&public_key),
        requested_capabilities: vec!["synchronize".to_string()],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        expires_at: "2030-01-01T00:00:00Z".to_string(),
    };
    let request = ActivationChallengeRequest {
        request_id: submission.request_id.clone(),
        tenant_id: tenant_id.to_string(),
        repository_id: repository_id.to_string(),
        proposed_node_id: node_id.clone(),
        public_key_fingerprint: submission.public_key_fingerprint.clone(),
    };
    let now = SystemTime::now();
    let mut enrollment = EnrollmentStore::connect(database_url)
        .await
        .expect("the test database should accept enrollment connections");
    enrollment
        .submit(&submission)
        .await
        .expect("the repository enrollment should be recorded");
    enrollment
        .approve(&EnrollmentApproval {
            request_id: submission.request_id.clone(),
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            public_key_fingerprint: submission.public_key_fingerprint.clone(),
            approved_capabilities: submission.requested_capabilities.clone(),
            approved_by: "administration-recovery-integration-administrator".to_string(),
        })
        .await
        .expect("the repository enrollment should be approved");
    let nonce: [u8; 32] = Sha256::digest(format!("{suffix}-{label}").as_bytes()).into();
    let challenge = enrollment
        .issue_challenge(&request, &nonce, now)
        .await
        .expect("the approved repository should receive an activation challenge");
    let signature = signing_key.sign(&activation_challenge_bytes(
        &challenge.nonce,
        &request.request_id,
        &request.tenant_id,
        &request.repository_id,
        &request.proposed_node_id,
        &request.public_key_fingerprint,
    ));
    enrollment
        .activate(
            &EnrollmentActivation {
                request,
                nonce: challenge.nonce,
                signature: signature.to_bytes().to_vec(),
            },
            &format!("administration-recovery-{label}-receipt-{suffix}"),
            &signing_key_id,
            now,
        )
        .await
        .expect("the repository enrollment should activate");
    SeedSigner::new(signing_key_id, node_id, &seed)
}

async fn administration_router(
    database_url: &str,
    tenant_id: &str,
    snapshot: Option<Arc<SnapshotProviderConfig>>,
) -> axum::Router {
    let fleet = Arc::new(
        FleetStore::connect(database_url)
            .await
            .expect("the test database should accept Fleet connections"),
    );
    let db_pool = ackplane_server::db_pool::build_pool(
        database_url,
        ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
    )
    .expect("the test pool builds from the gated database url");
    let administration = AdministrationStore::connect(&db_pool)
        .await
        .expect("the test database should accept Administration store connections");
    let claims = ClaimStore::connect(&db_pool)
        .await
        .expect("the test database should accept Claim connections");
    administration_routes(AdministrationApiState::with_claims(
        fleet,
        Arc::from(tenant_id.to_owned()),
        Arc::new(administration),
        Arc::new(claims),
        snapshot,
        None,
    ))
}

async fn post_json(router: &axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("the request should build"),
        )
        .await
        .expect("the request should receive a response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the response body should be bounded");
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("a non-empty response body should be JSON")
    };
    (status, json)
}

async fn get_json(router: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("the request should build"),
        )
        .await
        .expect("the request should receive a response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the response body should be bounded");
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("a non-empty response body should be JSON")
    };
    (status, json)
}

fn authentication_json(authentication: ackplane_protocol::v1::ClaimAuthentication) -> Value {
    json!({
        "signing_key_id": authentication.signing_key_id,
        "node_id": authentication.node_id,
        "signed_at": authentication.signed_at,
        "nonce": authentication.nonce,
        "signature": authentication.signature,
    })
}

#[allow(clippy::too_many_arguments)]
fn preview_body(
    signer: &SeedSigner,
    tenant_id: &str,
    repository_id: &str,
    policy_id: &str,
    snapshot_policy_id: &str,
    artifact_request_id: &str,
    manifest_digest: &[u8],
    safety_snapshot_idempotency_key: String,
    rehearsal_id: &str,
    idempotency_key: String,
) -> Value {
    let authentication = authenticate_recovery_execution(
        signer,
        tenant_id,
        repository_id,
        &RecoveryExecutionOperation::Preview {
            artifact_request_id,
            manifest_digest,
            safety_snapshot_idempotency_key: &safety_snapshot_idempotency_key,
            rehearsal_id,
            confirmation_window_seconds: CONFIRMATION_WINDOW_SECONDS,
            idempotency_key: &idempotency_key,
        },
    );
    json!({
        "policy_id": policy_id,
        "snapshot_policy_id": snapshot_policy_id,
        "repository_id": repository_id,
        "artifact_request_id": artifact_request_id,
        "manifest_digest_hex": hex_encode(manifest_digest),
        "safety_snapshot_idempotency_key": safety_snapshot_idempotency_key,
        "rehearsal_id": rehearsal_id,
        "confirmation_window_seconds": CONFIRMATION_WINDOW_SECONDS,
        "idempotency_key": idempotency_key,
        "authentication": authentication_json(authentication),
    })
}

fn confirmation_body(
    signer: &SeedSigner,
    tenant_id: &str,
    repository_id: &str,
    request_id: &str,
) -> Value {
    let authentication = authenticate_recovery_execution(
        signer,
        tenant_id,
        repository_id,
        &RecoveryExecutionOperation::Confirm { request_id },
    );
    json!({
        "repository_id": repository_id,
        "authentication": authentication_json(authentication),
    })
}

async fn adopt_policy(
    administration: &AdministrationStore,
    operation: AdministrationOperation,
    idempotency_key: String,
) -> String {
    let now = SystemTime::now();
    administration
        .adopt_policy(&PolicyAdoptionRequest {
            operation,
            scope: AdministrationScope::Platform,
            data_classification: "operational-metadata".to_string(),
            retention_basis: "self-hosted operator retention, ADR-0145 decision 4".to_string(),
            adopted_by: "administration-recovery-integration".to_string(),
            idempotency_key,
            effective_at: now,
            expires_at: now + Duration::from_secs(3600),
        })
        .await
        .expect("policy adoption should succeed")
        .policy
        .policy_id
}

/// ADR-0145 decision 5's own words: the safety Snapshot "is not optional and
/// its failure fails the preview." A `pg_dump_path` that cannot even spawn is
/// a deterministic, environment-independent way to exercise that failure
/// without depending on `pg_dump` actually being on `PATH` -- unlike the
/// happy-path test below, this one runs identically everywhere.
#[tokio::test]
async fn safety_snapshot_failure_fails_the_recovery_preview() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let suffix = unique_suffix();
    let tenant_id = format!("tenant-administration-recovery-safety-fail-{suffix}");
    let repository_id = format!("repository-administration-recovery-safety-fail-{suffix}");
    let requester = enroll_signer(
        &database_url,
        &tenant_id,
        &repository_id,
        &suffix,
        "requester",
        [31; 32],
    )
    .await;

    let administration = AdministrationStore::connect(
        &ackplane_server::db_pool::build_pool(
            &database_url,
            ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
        )
        .expect("the test pool builds from the gated database url"),
    )
    .await
    .expect("the test database should accept Administration store connections");
    let snapshot_policy_id = adopt_policy(
        &administration,
        AdministrationOperation::Snapshot,
        format!("snapshot-policy-{suffix}"),
    )
    .await;
    let recovery_policy_id = adopt_policy(
        &administration,
        AdministrationOperation::RecoveryExecution,
        format!("recovery-policy-{suffix}"),
    )
    .await;

    let dir = std::env::temp_dir().join(format!("ackplane-recovery-safety-fail-{suffix}"));
    let router = administration_router(
        &database_url,
        &tenant_id,
        Some(Arc::new(SnapshotProviderConfig {
            database_url: database_url.clone(),
            snapshot_dir: dir.clone(),
            key_path: dir.join("key.bin"),
            // Deliberately not a real binary: the safety Snapshot's own
            // `pg_dump` invocation fails to spawn, so the recovery preview
            // it gates must fail too -- deterministically, on every OS,
            // regardless of whether real `pg_dump` happens to be installed.
            pg_dump_path: "definitely-not-a-real-pg-dump-binary".to_string(),
            pg_restore_path: "pg_restore".to_string(),
            rehearsal_database_url: None,
            single_tenant_attested: false,
        })),
    )
    .await;

    let (status, body) = post_json(
        &router,
        "/api/v1/administration/recovery-executions",
        preview_body(
            &requester,
            &tenant_id,
            &repository_id,
            &recovery_policy_id,
            &snapshot_policy_id,
            &format!("artifact-request-{suffix}"),
            &[7_u8; 32],
            format!("safety-snapshot-{suffix}"),
            &format!("rehearsal-{suffix}"),
            format!("recovery-preview-{suffix}"),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "body was {body:?}");

    // No recovery-execution request must exist for a preview whose safety
    // point could not be captured.
    let (status, _) = get_json(
        &router,
        &format!("/api/v1/administration/recovery-executions/recovery-preview-{suffix}"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// The full happy path: a fresh safety Snapshot really runs, the preview
/// records the explicit impact plan, and a second, distinct enrolled key
/// authorizes the confirmation -- never executing `pg_restore` itself
/// (ADR-0145 decision 4, slice 4's own concern).
#[tokio::test]
async fn distinct_enrolled_keys_preview_and_confirm_a_recovery_execution() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    if tokio::process::Command::new("pg_dump")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .is_err()
    {
        println!("skipped: pg_dump is not available on PATH");
        return;
    }
    let suffix = unique_suffix();
    let tenant_id = format!("tenant-administration-recovery-{suffix}");
    let repository_id = format!("repository-administration-recovery-{suffix}");
    let requester = enroll_signer(
        &database_url,
        &tenant_id,
        &repository_id,
        &suffix,
        "requester",
        [32; 32],
    )
    .await;
    let confirmer = enroll_signer(
        &database_url,
        &tenant_id,
        &repository_id,
        &suffix,
        "confirmer",
        [33; 32],
    )
    .await;

    let administration = AdministrationStore::connect(
        &ackplane_server::db_pool::build_pool(
            &database_url,
            ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
        )
        .expect("the test pool builds from the gated database url"),
    )
    .await
    .expect("the test database should accept Administration store connections");
    let snapshot_policy_id = adopt_policy(
        &administration,
        AdministrationOperation::Snapshot,
        format!("snapshot-policy-{suffix}"),
    )
    .await;
    let recovery_policy_id = adopt_policy(
        &administration,
        AdministrationOperation::RecoveryExecution,
        format!("recovery-policy-{suffix}"),
    )
    .await;

    let dir = std::env::temp_dir().join(format!("ackplane-recovery-integration-{suffix}"));
    let _ = std::fs::remove_dir_all(&dir);
    let snapshot_config = Arc::new(SnapshotProviderConfig {
        database_url: database_url.clone(),
        snapshot_dir: dir.clone(),
        key_path: dir.join("key.bin"),
        pg_dump_path: "pg_dump".to_string(),
        pg_restore_path: "pg_restore".to_string(),
        rehearsal_database_url: None,
        single_tenant_attested: false,
    });
    let router =
        administration_router(&database_url, &tenant_id, Some(snapshot_config.clone())).await;

    // The artifact being restored: an ordinary platform Snapshot, executed
    // for real, exactly like the Snapshot feature's own integration test.
    let artifact = ackplane_server::snapshot_provider::create_platform_snapshot(
        &snapshot_config,
        &format!("artifact-request-{suffix}"),
    )
    .await
    .expect("creating the artifact snapshot should succeed");
    let artifact_request = administration
        .request_snapshot(
            &ackplane_server::administration_store::NewSnapshotRequest {
                policy_id: snapshot_policy_id.clone(),
                requested_by: "administration-recovery-integration".to_string(),
                scope: AdministrationScope::Platform,
                idempotency_key: format!("artifact-snapshot-{suffix}"),
            },
            SystemTime::now(),
        )
        .await
        .expect("the artifact snapshot request should succeed")
        .request;
    administration
        .record_snapshot_receipt(
            &ackplane_server::administration_store::NewSnapshotReceipt {
                request_id: artifact_request.request_id.clone(),
                outcome: ackplane_server::administration_store::SnapshotOutcome::Succeeded,
                reason: "pg_dump completed and the artifact was encrypted.".to_string(),
                artifact_path: Some(artifact.artifact_path),
                manifest_digest: Some(artifact.manifest_digest.clone()),
                encryption_key_id: Some(artifact.encryption_key_id),
                size_bytes: Some(artifact.size_bytes),
                verified: true,
                occurred_at: SystemTime::now(),
            },
            SystemTime::now(),
        )
        .await
        .expect("recording the artifact receipt should succeed");
    administration
        .record_recovery_rehearsal(
            &NewRecoveryRehearsal {
                request_id: artifact_request.request_id.clone(),
                requested_by: "administration-recovery-integration".to_string(),
                manifest_digest: artifact.manifest_digest.clone(),
                restore_duration_ms: 1_200,
                migration_version_matched: true,
                archive_table_count: Some(1),
                restored_table_count: Some(1),
                restored_row_count: Some(1),
                passed: true,
                reason: "rehearsal passed".to_string(),
                occurred_at: SystemTime::now(),
            },
            SystemTime::now(),
        )
        .await
        .expect("recording the passing rehearsal should succeed");
    let rehearsal_id = administration
        .latest_passing_recovery_rehearsal(&artifact.manifest_digest)
        .await
        .expect("reading the rehearsal back should succeed")
        .expect("the just-recorded rehearsal should be found")
        .rehearsal_id;

    let (preview_status, preview_body_json) = post_json(
        &router,
        "/api/v1/administration/recovery-executions",
        preview_body(
            &requester,
            &tenant_id,
            &repository_id,
            &recovery_policy_id,
            &snapshot_policy_id,
            &artifact_request.request_id,
            &artifact.manifest_digest,
            format!("safety-snapshot-{suffix}"),
            &rehearsal_id,
            format!("recovery-preview-{suffix}"),
        ),
    )
    .await;
    assert_eq!(
        preview_status,
        StatusCode::OK,
        "body was {preview_body_json:?}"
    );
    assert_eq!(
        preview_body_json["artifact_request_id"],
        json!(artifact_request.request_id)
    );
    assert_eq!(preview_body_json["rehearsal_id"], json!(rehearsal_id));
    let request_id = preview_body_json["request_id"]
        .as_str()
        .expect("the response should carry a request_id")
        .to_string();

    // The requester's own key must not be able to confirm its own preview.
    let (self_confirm_status, self_confirm_body) = post_json(
        &router,
        &format!("/api/v1/administration/recovery-executions/{request_id}/confirm"),
        confirmation_body(&requester, &tenant_id, &repository_id, &request_id),
    )
    .await;
    assert_eq!(
        self_confirm_status,
        StatusCode::FORBIDDEN,
        "body was {self_confirm_body:?}"
    );

    let (confirm_status, confirm_body) = post_json(
        &router,
        &format!("/api/v1/administration/recovery-executions/{request_id}/confirm"),
        confirmation_body(&confirmer, &tenant_id, &repository_id, &request_id),
    )
    .await;
    assert_eq!(confirm_status, StatusCode::OK, "body was {confirm_body:?}");
    assert_eq!(confirm_body["outcome"], json!("confirmed"));
    assert_eq!(
        confirm_body["confirming_node_id"],
        json!(confirmer.node_id())
    );

    let (status, body) = get_json(
        &router,
        &format!("/api/v1/administration/recovery-executions/{request_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body was {body:?}");
    assert_eq!(body["request_id"], json!(request_id));

    let _ = std::fs::remove_dir_all(&dir);
}

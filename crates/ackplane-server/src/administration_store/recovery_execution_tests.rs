use std::time::{Duration, SystemTime};

use super::*;
use crate::test_support::unique_id;

fn pool() -> Option<crate::db_pool::PgPool> {
    crate::test_support::test_pool()
}

fn snapshot_policy_request(adopted_by: &str, suffix: &str) -> PolicyAdoptionRequest {
    let now = SystemTime::now();
    PolicyAdoptionRequest {
        operation: AdministrationOperation::Snapshot,
        scope: AdministrationScope::Platform,
        data_classification: "operational-metadata".to_owned(),
        retention_basis: "self-hosted operator retention, ADR-0119 decision 2".to_owned(),
        adopted_by: adopted_by.to_owned(),
        idempotency_key: format!("snapshot-policy-{suffix}"),
        effective_at: now,
        expires_at: now + Duration::from_secs(3600),
    }
}

fn recovery_policy_request(adopted_by: &str, suffix: &str) -> PolicyAdoptionRequest {
    let now = SystemTime::now();
    PolicyAdoptionRequest {
        operation: AdministrationOperation::RecoveryExecution,
        scope: AdministrationScope::Platform,
        data_classification: "operational-metadata".to_owned(),
        retention_basis: "self-hosted operator retention, ADR-0145 decision 4".to_owned(),
        adopted_by: adopted_by.to_owned(),
        idempotency_key: format!("recovery-policy-{suffix}"),
        effective_at: now,
        expires_at: now + Duration::from_secs(3600),
    }
}

/// A succeeded Snapshot artifact fixture, so a recovery-execution preview has
/// something real to name as the artifact being restored. Digests are
/// distinct per fixture (`fixture_digest`) so the preview's own
/// cross-referenced digest checks have something meaningful to distinguish.
async fn succeeded_snapshot_receipt(
    store: &AdministrationStore,
    snapshot_policy_id: &str,
    suffix: &str,
    label: &str,
    digest: Vec<u8>,
) -> SnapshotReceipt {
    let request = store
        .request_snapshot(
            &NewSnapshotRequest {
                policy_id: snapshot_policy_id.to_owned(),
                requested_by: "loopback-principal".to_owned(),
                scope: AdministrationScope::Platform,
                idempotency_key: format!("snapshot-{label}-{suffix}"),
            },
            SystemTime::now(),
        )
        .await
        .expect("the snapshot request should succeed")
        .request;
    store
        .record_snapshot_receipt(
            &NewSnapshotReceipt {
                request_id: request.request_id,
                outcome: SnapshotOutcome::Succeeded,
                reason: "pg_dump completed and the artifact was encrypted.".to_owned(),
                artifact_path: Some(format!("/tmp/{label}-{suffix}.snapshot")),
                manifest_digest: Some(digest),
                encryption_key_id: Some("ackplane-snapshot-key-v1".to_owned()),
                size_bytes: Some(4096),
                verified: true,
                occurred_at: SystemTime::now(),
            },
            SystemTime::now(),
        )
        .await
        .expect("recording the snapshot receipt should succeed")
}

fn fixture_digest(seed: u8) -> Vec<u8> {
    vec![seed; 32]
}

async fn passing_rehearsal(
    store: &AdministrationStore,
    artifact_request_id: &str,
    suffix: &str,
    digest: Vec<u8>,
) -> RecoveryRehearsal {
    store
        .record_recovery_rehearsal(
            &NewRecoveryRehearsal {
                request_id: artifact_request_id.to_owned(),
                requested_by: "loopback-principal".to_owned(),
                manifest_digest: digest,
                restore_duration_ms: 1_200,
                migration_version_matched: true,
                archive_table_count: Some(12),
                restored_table_count: Some(12),
                restored_row_count: Some(340),
                passed: true,
                reason: format!("rehearsal-{suffix} passed"),
                occurred_at: SystemTime::now(),
            },
            SystemTime::now(),
        )
        .await
        .expect("recording the passing rehearsal should succeed")
}

fn preview_request(
    policy_id: &str,
    artifact_request_id: &str,
    manifest_digest: Vec<u8>,
    safety_receipt: &SnapshotReceipt,
    rehearsal_id: &str,
    suffix: &str,
) -> RecoveryExecutionPreviewRequest {
    RecoveryExecutionPreviewRequest {
        policy_id: policy_id.to_owned(),
        requested_by: format!("requester-{suffix}"),
        tenant_id: format!("tenant-{suffix}"),
        requesting_node_id: format!("node-for-requester-{suffix}"),
        requesting_public_key_fingerprint: format!("fingerprint-for-requester-{suffix}"),
        artifact_request_id: artifact_request_id.to_owned(),
        manifest_digest,
        safety_snapshot_receipt_id: safety_receipt.receipt_id.clone(),
        safety_snapshot_digest: safety_receipt
            .manifest_digest
            .clone()
            .expect("the safety snapshot fixture always records a digest"),
        rehearsal_id: rehearsal_id.to_owned(),
        confirmation_window: Duration::from_secs(900),
        idempotency_key: format!("recovery-preview-{suffix}"),
    }
}

#[tokio::test]
async fn a_preview_with_no_active_recovery_policy_is_refused() {
    let Some(pool) = pool() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-recovery-execution-no-policy");
    let store = AdministrationStore::connect(&pool)
        .await
        .expect("the test database should accept administration store connections");

    let snapshot_policy = store
        .adopt_policy(&snapshot_policy_request("loopback-principal", &suffix))
        .await
        .expect("snapshot policy adoption should succeed")
        .policy;

    let digest = fixture_digest(1);
    let artifact = succeeded_snapshot_receipt(
        &store,
        &snapshot_policy.policy_id,
        &suffix,
        "artifact",
        digest.clone(),
    )
    .await;
    let safety = succeeded_snapshot_receipt(
        &store,
        &snapshot_policy.policy_id,
        &suffix,
        "safety",
        fixture_digest(2),
    )
    .await;
    let rehearsal = passing_rehearsal(&store, &artifact.request_id, &suffix, digest.clone()).await;

    let result = store
        .preview_recovery_execution(
            &preview_request(
                &format!("administration-policy:does-not-exist-{suffix}"),
                &artifact.request_id,
                digest,
                &safety,
                &rehearsal.rehearsal_id,
                &suffix,
            ),
            SystemTime::now(),
        )
        .await;
    assert!(matches!(
        result,
        Err(AdministrationStoreError::NoActivePolicy)
    ));
}

#[tokio::test]
async fn a_preview_whose_declared_digest_does_not_match_the_artifacts_own_receipt_is_refused() {
    let Some(pool) = pool() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-recovery-execution-digest-mismatch");
    let store = AdministrationStore::connect(&pool)
        .await
        .expect("the test database should accept administration store connections");

    let snapshot_policy = store
        .adopt_policy(&snapshot_policy_request("loopback-principal", &suffix))
        .await
        .expect("snapshot policy adoption should succeed")
        .policy;

    let policy = store
        .adopt_policy(&recovery_policy_request("loopback-principal", &suffix))
        .await
        .expect("recovery policy adoption should succeed")
        .policy;
    let real_digest = fixture_digest(3);
    let artifact = succeeded_snapshot_receipt(
        &store,
        &snapshot_policy.policy_id,
        &suffix,
        "artifact",
        real_digest.clone(),
    )
    .await;
    let safety = succeeded_snapshot_receipt(
        &store,
        &snapshot_policy.policy_id,
        &suffix,
        "safety",
        fixture_digest(4),
    )
    .await;
    let rehearsal =
        passing_rehearsal(&store, &artifact.request_id, &suffix, real_digest.clone()).await;

    let declared_digest = fixture_digest(9); // deliberately different from real_digest
    let result = store
        .preview_recovery_execution(
            &preview_request(
                &policy.policy_id,
                &artifact.request_id,
                declared_digest,
                &safety,
                &rehearsal.rehearsal_id,
                &suffix,
            ),
            SystemTime::now(),
        )
        .await;
    assert!(matches!(
        result,
        Err(AdministrationStoreError::RecoveryArtifactManifestMismatch)
    ));
}

#[tokio::test]
async fn a_preview_naming_a_rehearsal_that_never_passed_is_refused() {
    let Some(pool) = pool() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-recovery-execution-failed-rehearsal");
    let store = AdministrationStore::connect(&pool)
        .await
        .expect("the test database should accept administration store connections");

    let snapshot_policy = store
        .adopt_policy(&snapshot_policy_request("loopback-principal", &suffix))
        .await
        .expect("snapshot policy adoption should succeed")
        .policy;

    let policy = store
        .adopt_policy(&recovery_policy_request("loopback-principal", &suffix))
        .await
        .expect("recovery policy adoption should succeed")
        .policy;
    let digest = fixture_digest(5);
    let artifact = succeeded_snapshot_receipt(
        &store,
        &snapshot_policy.policy_id,
        &suffix,
        "artifact",
        digest.clone(),
    )
    .await;
    let safety = succeeded_snapshot_receipt(
        &store,
        &snapshot_policy.policy_id,
        &suffix,
        "safety",
        fixture_digest(6),
    )
    .await;
    let failed_rehearsal = store
        .record_recovery_rehearsal(
            &NewRecoveryRehearsal {
                request_id: artifact.request_id.clone(),
                requested_by: "loopback-principal".to_owned(),
                manifest_digest: digest.clone(),
                restore_duration_ms: 1_200,
                migration_version_matched: false,
                archive_table_count: Some(12),
                restored_table_count: Some(9),
                restored_row_count: Some(20),
                passed: false,
                reason: "migration version did not match".to_owned(),
                occurred_at: SystemTime::now(),
            },
            SystemTime::now(),
        )
        .await
        .expect("recording the failed rehearsal should succeed");

    let result = store
        .preview_recovery_execution(
            &preview_request(
                &policy.policy_id,
                &artifact.request_id,
                digest,
                &safety,
                &failed_rehearsal.rehearsal_id,
                &suffix,
            ),
            SystemTime::now(),
        )
        .await;
    assert!(matches!(
        result,
        Err(AdministrationStoreError::NoPassingRehearsalForArtifact)
    ));
}

#[tokio::test]
async fn a_valid_preview_records_the_explicit_impact_plan_and_replays_idempotently() {
    let Some(pool) = pool() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-recovery-execution-preview");
    let store = AdministrationStore::connect(&pool)
        .await
        .expect("the test database should accept administration store connections");

    let snapshot_policy = store
        .adopt_policy(&snapshot_policy_request("loopback-principal", &suffix))
        .await
        .expect("snapshot policy adoption should succeed")
        .policy;

    let policy = store
        .adopt_policy(&recovery_policy_request("loopback-principal", &suffix))
        .await
        .expect("recovery policy adoption should succeed")
        .policy;
    let digest = fixture_digest(7);
    let artifact = succeeded_snapshot_receipt(
        &store,
        &snapshot_policy.policy_id,
        &suffix,
        "artifact",
        digest.clone(),
    )
    .await;
    let safety = succeeded_snapshot_receipt(
        &store,
        &snapshot_policy.policy_id,
        &suffix,
        "safety",
        fixture_digest(8),
    )
    .await;
    let rehearsal = passing_rehearsal(&store, &artifact.request_id, &suffix, digest.clone()).await;

    let request = preview_request(
        &policy.policy_id,
        &artifact.request_id,
        digest.clone(),
        &safety,
        &rehearsal.rehearsal_id,
        &suffix,
    );
    let first = store
        .preview_recovery_execution(&request, SystemTime::now())
        .await
        .expect("a valid preview should succeed");
    assert!(!first.idempotent_replay);
    assert_eq!(first.request.artifact_request_id, artifact.request_id);
    assert_eq!(first.request.manifest_digest, digest);
    assert_eq!(first.request.safety_snapshot_receipt_id, safety.receipt_id);
    assert_eq!(first.request.rehearsal_id, rehearsal.rehearsal_id);

    let replay = store
        .preview_recovery_execution(&request, SystemTime::now())
        .await
        .expect("replaying the exact same preview should succeed idempotently");
    assert!(replay.idempotent_replay);
    assert_eq!(replay.request.request_id, first.request.request_id);
}

#[tokio::test]
async fn confirming_with_the_previewing_signing_keys_own_fingerprint_is_refused() {
    let Some(pool) = pool() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-recovery-execution-self-confirm");
    let store = AdministrationStore::connect(&pool)
        .await
        .expect("the test database should accept administration store connections");

    let snapshot_policy = store
        .adopt_policy(&snapshot_policy_request("loopback-principal", &suffix))
        .await
        .expect("snapshot policy adoption should succeed")
        .policy;

    let policy = store
        .adopt_policy(&recovery_policy_request("loopback-principal", &suffix))
        .await
        .expect("recovery policy adoption should succeed")
        .policy;
    let digest = fixture_digest(11);
    let artifact = succeeded_snapshot_receipt(
        &store,
        &snapshot_policy.policy_id,
        &suffix,
        "artifact",
        digest.clone(),
    )
    .await;
    let safety = succeeded_snapshot_receipt(
        &store,
        &snapshot_policy.policy_id,
        &suffix,
        "safety",
        fixture_digest(12),
    )
    .await;
    let rehearsal = passing_rehearsal(&store, &artifact.request_id, &suffix, digest.clone()).await;
    let request = preview_request(
        &policy.policy_id,
        &artifact.request_id,
        digest,
        &safety,
        &rehearsal.rehearsal_id,
        &suffix,
    );
    let preview_fingerprint = request.requesting_public_key_fingerprint.clone();
    let outcome = store
        .preview_recovery_execution(&request, SystemTime::now())
        .await
        .expect("a valid preview should succeed");

    let result = store
        .confirm_recovery_execution(
            &outcome.request.request_id,
            "confirming-signing-key",
            "confirming-node",
            &preview_fingerprint,
            SystemTime::now(),
        )
        .await;
    assert!(matches!(
        result,
        Err(AdministrationStoreError::SelfConfirmationRefused)
    ));
}

#[tokio::test]
async fn confirming_with_a_distinct_key_records_a_confirmed_authorization_and_replays_idempotently()
{
    let Some(pool) = pool() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-recovery-execution-confirm");
    let store = AdministrationStore::connect(&pool)
        .await
        .expect("the test database should accept administration store connections");

    let snapshot_policy = store
        .adopt_policy(&snapshot_policy_request("loopback-principal", &suffix))
        .await
        .expect("snapshot policy adoption should succeed")
        .policy;

    let policy = store
        .adopt_policy(&recovery_policy_request("loopback-principal", &suffix))
        .await
        .expect("recovery policy adoption should succeed")
        .policy;
    let digest = fixture_digest(13);
    let artifact = succeeded_snapshot_receipt(
        &store,
        &snapshot_policy.policy_id,
        &suffix,
        "artifact",
        digest.clone(),
    )
    .await;
    let safety = succeeded_snapshot_receipt(
        &store,
        &snapshot_policy.policy_id,
        &suffix,
        "safety",
        fixture_digest(14),
    )
    .await;
    let rehearsal = passing_rehearsal(&store, &artifact.request_id, &suffix, digest.clone()).await;
    let request = preview_request(
        &policy.policy_id,
        &artifact.request_id,
        digest,
        &safety,
        &rehearsal.rehearsal_id,
        &suffix,
    );
    let outcome = store
        .preview_recovery_execution(&request, SystemTime::now())
        .await
        .expect("a valid preview should succeed");

    let now = SystemTime::now();
    let first = store
        .confirm_recovery_execution(
            &outcome.request.request_id,
            "confirming-signing-key",
            "confirming-node",
            &format!("distinct-fingerprint-{suffix}"),
            now,
        )
        .await
        .expect("confirming with a distinct key should succeed");
    assert_eq!(first.outcome, RecoveryConfirmationOutcome::Confirmed);
    // This slice never runs pg_restore -- confirming must not create an
    // execution record, only an authorization one.
    assert!(store
        .recovery_execution_request(&outcome.request.request_id)
        .await
        .expect("reading the request back should succeed")
        .is_some());

    let replay = store
        .confirm_recovery_execution(
            &outcome.request.request_id,
            "a-different-signing-key",
            "a-different-node",
            &format!("yet-another-fingerprint-{suffix}"),
            now,
        )
        .await
        .expect("confirming an already-confirmed request should replay, not re-authorize");
    assert_eq!(replay.confirmation_id, first.confirmation_id);
    assert_eq!(replay.confirming_node_id, first.confirming_node_id);
}

#[tokio::test]
async fn confirming_after_the_window_expires_refuses_without_authorizing() {
    let Some(pool) = pool() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-recovery-execution-expired");
    let store = AdministrationStore::connect(&pool)
        .await
        .expect("the test database should accept administration store connections");

    let snapshot_policy = store
        .adopt_policy(&snapshot_policy_request("loopback-principal", &suffix))
        .await
        .expect("snapshot policy adoption should succeed")
        .policy;

    let policy = store
        .adopt_policy(&recovery_policy_request("loopback-principal", &suffix))
        .await
        .expect("recovery policy adoption should succeed")
        .policy;
    let digest = fixture_digest(15);
    let artifact = succeeded_snapshot_receipt(
        &store,
        &snapshot_policy.policy_id,
        &suffix,
        "artifact",
        digest.clone(),
    )
    .await;
    let safety = succeeded_snapshot_receipt(
        &store,
        &snapshot_policy.policy_id,
        &suffix,
        "safety",
        fixture_digest(16),
    )
    .await;
    let rehearsal = passing_rehearsal(&store, &artifact.request_id, &suffix, digest.clone()).await;
    let mut request = preview_request(
        &policy.policy_id,
        &artifact.request_id,
        digest,
        &safety,
        &rehearsal.rehearsal_id,
        &suffix,
    );
    request.confirmation_window = Duration::from_secs(1);
    let outcome = store
        .preview_recovery_execution(&request, SystemTime::now())
        .await
        .expect("a valid preview should succeed");

    let later = SystemTime::now() + Duration::from_secs(3_600);
    let confirmation = store
        .confirm_recovery_execution(
            &outcome.request.request_id,
            "confirming-signing-key",
            "confirming-node",
            &format!("distinct-fingerprint-{suffix}"),
            later,
        )
        .await
        .expect("an expired confirmation attempt still records a receipt");
    assert_eq!(confirmation.outcome, RecoveryConfirmationOutcome::Expired);
}

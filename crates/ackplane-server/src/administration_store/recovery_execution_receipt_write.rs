//! ADR-0145 decision 4-7: executes a previously confirmed recovery-execution
//! request -- the one place that runs `pg_restore` against the authoritative
//! `ACKPLANE_DATABASE_URL` for real. Split from `recovery_execution_write.rs`
//! (preview/confirm, ADR-0145 decision 4-5): that file's confirmation is an
//! *authorization* only and never touches production; this is the distinct,
//! later step that actually consumes a `Confirmed` authorization.

use std::time::SystemTime;

use crate::snapshot_provider::{self, SnapshotProviderConfig, SnapshotProviderError};

use super::recovery_execution_model::RecoveryConfirmationOutcome;
use super::recovery_execution_receipt_model::{
    assigned_receipt_id, receipt_from_row, validate_receipt, NewRecoveryExecutionReceipt,
    RecoveryExecutionOutcome, RecoveryExecutionReceipt,
};
use super::{AdministrationStore, AdministrationStoreError};

impl AdministrationStore {
    /// Executes (or refuses) a previously previewed and confirmed recovery
    /// request. Idempotent: a request that already has a receipt returns it
    /// unchanged, never re-running `pg_restore` a second time.
    ///
    /// Refuses -- recording a durable `Refused` receipt, never a bare error
    /// -- when: the request has no `Confirmed` authorization; the rehearsal
    /// it names is missing, did not pass, covers a different artifact digest,
    /// or has aged past [`snapshot_provider::MAX_REHEARSAL_FRESHNESS`]; or the
    /// deployment is not attested single-tenant. A genuine `pg_restore`
    /// failure is a distinct `Failed` receipt, never conflated with a
    /// pre-flight refusal (ADR-0119 decision 10: no silent fallback, no retry
    /// with checks relaxed).
    pub async fn execute_recovery(
        &self,
        request_id: &str,
        config: &SnapshotProviderConfig,
        now: SystemTime,
    ) -> Result<RecoveryExecutionReceipt, AdministrationStoreError> {
        if let Some(existing) = self
            .recovery_execution_receipt_for_request(request_id)
            .await?
        {
            return Ok(existing);
        }

        let request = self
            .recovery_execution_request(request_id)
            .await?
            .ok_or_else(
                || AdministrationStoreError::UnknownRecoveryExecutionRequest {
                    request_id: request_id.to_string(),
                },
            )?;
        let confirmation = self
            .recovery_confirmation_for_request(request_id)
            .await?
            .filter(|confirmation| confirmation.outcome == RecoveryConfirmationOutcome::Confirmed)
            .ok_or(AdministrationStoreError::RecoveryExecutionNotConfirmed)?;
        let (confirming_node_id, confirming_public_key_fingerprint) = match (
            confirmation.confirming_node_id.clone(),
            confirmation.confirming_public_key_fingerprint.clone(),
        ) {
            (Some(node_id), Some(fingerprint)) => (node_id, fingerprint),
            _ => return Err(AdministrationStoreError::RecoveryExecutionNotConfirmed),
        };

        let new_receipt =
            |outcome: RecoveryExecutionOutcome, reason: String| NewRecoveryExecutionReceipt {
                request_id: request_id.to_string(),
                tenant_id: request.tenant_id.clone(),
                old_manifest_digest: request.safety_snapshot_digest.clone(),
                new_manifest_digest: request.manifest_digest.clone(),
                rehearsal_id: request.rehearsal_id.clone(),
                previewing_node_id: request.requesting_node_id.clone(),
                previewing_public_key_fingerprint: request
                    .requesting_public_key_fingerprint
                    .clone(),
                confirming_node_id: confirming_node_id.clone(),
                confirming_public_key_fingerprint: confirming_public_key_fingerprint.clone(),
                outcome,
                reason,
                occurred_at: now,
            };

        // Re-checked here, not only at preview time: the deployment's
        // attestation and the rehearsal's freshness are both facts that can
        // change between preview/confirm and this later, separate execution
        // step, and decision 6/3 both gate *execution*, not merely preview.
        if let Err(SnapshotProviderError::MultiTenantRecoveryUnavailable) =
            config.ensure_recovery_execution_permitted()
        {
            return self
                .record_recovery_execution_receipt(new_receipt(
                    RecoveryExecutionOutcome::Refused,
                    "This deployment is not attested single-tenant \
                     (ACKPLANE_SINGLE_TENANT_ATTESTED is not true)."
                        .to_string(),
                ))
                .await;
        }

        let rehearsal = self.recovery_rehearsal(&request.rehearsal_id).await?;
        let rehearsal_valid = match &rehearsal {
            Some(rehearsal) => {
                rehearsal.passed
                    && rehearsal.manifest_digest == request.manifest_digest
                    && snapshot_provider::rehearsal_is_fresh(rehearsal.occurred_at, now)
            }
            None => false,
        };
        if !rehearsal_valid {
            return self
                .record_recovery_execution_receipt(new_receipt(
                    RecoveryExecutionOutcome::Refused,
                    "The named rehearsal report does not exist, did not pass, covers a \
                     different artifact digest, or is older than the freshness window."
                        .to_string(),
                ))
                .await;
        }

        let artifact_receipt = self
            .snapshot_receipt_for_request(&request.artifact_request_id)
            .await?
            .ok_or(AdministrationStoreError::UnknownRecoveryArtifact)?;
        let Some(artifact_path) = artifact_receipt.artifact_path else {
            return Err(AdministrationStoreError::UnknownRecoveryArtifact);
        };

        let restore =
            snapshot_provider::execute_recovery(config, &artifact_path, &request.manifest_digest)
                .await;
        let (outcome, reason) = match restore {
            Ok(report) if report.succeeded => (RecoveryExecutionOutcome::Succeeded, report.reason),
            Ok(report) => (RecoveryExecutionOutcome::Failed, report.reason),
            Err(SnapshotProviderError::MultiTenantRecoveryUnavailable) => (
                RecoveryExecutionOutcome::Refused,
                "This deployment is not attested single-tenant \
                 (ACKPLANE_SINGLE_TENANT_ATTESTED is not true)."
                    .to_string(),
            ),
            Err(error) => (
                RecoveryExecutionOutcome::Failed,
                format!("the restore could not be attempted: {error}"),
            ),
        };
        self.record_recovery_execution_receipt(new_receipt(outcome, reason))
            .await
    }

    async fn record_recovery_execution_receipt(
        &self,
        new_receipt: NewRecoveryExecutionReceipt,
    ) -> Result<RecoveryExecutionReceipt, AdministrationStoreError> {
        validate_receipt(&new_receipt, new_receipt.occurred_at)?;
        let receipt_id = assigned_receipt_id(&new_receipt)?;
        let connection = self.connection().await?;
        let inserted = connection
            .query_opt(
                "INSERT INTO administration_recovery_execution_receipts (receipt_id, \
                     request_id, tenant_id, old_manifest_digest, new_manifest_digest, \
                     rehearsal_id, previewing_node_id, previewing_public_key_fingerprint, \
                     confirming_node_id, confirming_public_key_fingerprint, outcome, reason, \
                     occurred_at, recorded_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) \
                 ON CONFLICT DO NOTHING RETURNING *",
                &[
                    &receipt_id,
                    &new_receipt.request_id,
                    &new_receipt.tenant_id,
                    &new_receipt.old_manifest_digest,
                    &new_receipt.new_manifest_digest,
                    &new_receipt.rehearsal_id,
                    &new_receipt.previewing_node_id,
                    &new_receipt.previewing_public_key_fingerprint,
                    &new_receipt.confirming_node_id,
                    &new_receipt.confirming_public_key_fingerprint,
                    &new_receipt.outcome.as_i16(),
                    &new_receipt.reason,
                    &new_receipt.occurred_at,
                    &new_receipt.occurred_at,
                ],
            )
            .await?;
        match inserted {
            Some(row) => receipt_from_row(&row),
            // Lost the race to a concurrent attempt for the same request:
            // `UNIQUE (request_id)` means one already exists, and returning
            // it is correct -- execution already happened (or was already
            // refused), and this call must not attempt a second `pg_restore`.
            None => self
                .recovery_execution_receipt_for_request(&new_receipt.request_id)
                .await?
                .ok_or(AdministrationStoreError::ReceiptConflict),
        }
    }

    pub async fn recovery_execution_receipt_for_request(
        &self,
        request_id: &str,
    ) -> Result<Option<RecoveryExecutionReceipt>, AdministrationStoreError> {
        let connection = self.connection().await?;
        let row = connection
            .query_opt(
                "SELECT * FROM administration_recovery_execution_receipts WHERE request_id = $1",
                &[&request_id],
            )
            .await?;
        row.as_ref().map(receipt_from_row).transpose()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::administration_store::{
        AdministrationOperation, AdministrationScope, NewRecoveryRehearsal, NewSnapshotReceipt,
        NewSnapshotRequest, PolicyAdoptionRequest, RecoveryExecutionPreviewRequest,
        SnapshotOutcome,
    };
    use crate::test_support::unique_id;

    fn pool() -> Option<crate::db_pool::PgPool> {
        crate::test_support::test_pool()
    }

    fn dummy_config(single_tenant_attested: bool) -> SnapshotProviderConfig {
        SnapshotProviderConfig {
            database_url: "******localhost:5432/does-not-need-to-exist".to_string(),
            snapshot_dir: std::env::temp_dir(),
            key_path: std::env::temp_dir().join("unused-key.bin"),
            pg_dump_path: "pg_dump".to_string(),
            pg_restore_path: "pg_restore".to_string(),
            rehearsal_database_url: None,
            single_tenant_attested,
        }
    }

    fn fixture_digest(seed: u8) -> Vec<u8> {
        vec![seed; 32]
    }

    /// Everything a *previewed* (not yet confirmed) recovery-execution
    /// request needs: both policies adopted, a succeeded artifact Snapshot
    /// and a separate succeeded safety Snapshot, and a passing rehearsal of
    /// the artifact's exact digest. Shared by `confirmed_request` (which
    /// confirms on top) and the not-confirmed test (which deliberately does
    /// not).
    async fn previewed_request(store: &AdministrationStore, suffix: &str) -> String {
        let now = SystemTime::now();
        let snapshot_policy = store
            .adopt_policy(&PolicyAdoptionRequest {
                operation: AdministrationOperation::Snapshot,
                scope: AdministrationScope::Platform,
                data_classification: "operational-metadata".to_owned(),
                retention_basis: "test fixture".to_owned(),
                adopted_by: "loopback-principal".to_owned(),
                idempotency_key: format!("snapshot-policy-{suffix}"),
                effective_at: now,
                expires_at: now + Duration::from_secs(3600),
            })
            .await
            .expect("snapshot policy adoption should succeed")
            .policy;
        let recovery_policy = store
            .adopt_policy(&PolicyAdoptionRequest {
                operation: AdministrationOperation::RecoveryExecution,
                scope: AdministrationScope::Platform,
                data_classification: "operational-metadata".to_owned(),
                retention_basis: "test fixture".to_owned(),
                adopted_by: "loopback-principal".to_owned(),
                idempotency_key: format!("recovery-policy-{suffix}"),
                effective_at: now,
                expires_at: now + Duration::from_secs(3600),
            })
            .await
            .expect("recovery policy adoption should succeed")
            .policy;

        let digest = fixture_digest(1);
        let artifact_request = store
            .request_snapshot(
                &NewSnapshotRequest {
                    policy_id: snapshot_policy.policy_id.clone(),
                    requested_by: "loopback-principal".to_owned(),
                    scope: AdministrationScope::Platform,
                    idempotency_key: format!("artifact-snapshot-{suffix}"),
                },
                now,
            )
            .await
            .expect("the artifact snapshot request should succeed")
            .request;
        store
            .record_snapshot_receipt(
                &NewSnapshotReceipt {
                    request_id: artifact_request.request_id.clone(),
                    outcome: SnapshotOutcome::Succeeded,
                    reason: "fixture".to_owned(),
                    artifact_path: Some(format!("/tmp/artifact-{suffix}.snapshot")),
                    manifest_digest: Some(digest.clone()),
                    encryption_key_id: Some("ackplane-snapshot-key-v1".to_owned()),
                    size_bytes: Some(4096),
                    verified: true,
                    occurred_at: now,
                },
                now,
            )
            .await
            .expect("recording the artifact receipt should succeed");

        let safety_request = store
            .request_snapshot(
                &NewSnapshotRequest {
                    policy_id: snapshot_policy.policy_id,
                    requested_by: "loopback-principal".to_owned(),
                    scope: AdministrationScope::Platform,
                    idempotency_key: format!("safety-snapshot-{suffix}"),
                },
                now,
            )
            .await
            .expect("the safety snapshot request should succeed")
            .request;
        let safety_receipt = store
            .record_snapshot_receipt(
                &NewSnapshotReceipt {
                    request_id: safety_request.request_id,
                    outcome: SnapshotOutcome::Succeeded,
                    reason: "fixture".to_owned(),
                    artifact_path: Some(format!("/tmp/safety-{suffix}.snapshot")),
                    manifest_digest: Some(fixture_digest(2)),
                    encryption_key_id: Some("ackplane-snapshot-key-v1".to_owned()),
                    size_bytes: Some(4096),
                    verified: true,
                    occurred_at: now,
                },
                now,
            )
            .await
            .expect("recording the safety receipt should succeed");

        let rehearsal = store
            .record_recovery_rehearsal(
                &NewRecoveryRehearsal {
                    request_id: artifact_request.request_id.clone(),
                    requested_by: "loopback-principal".to_owned(),
                    manifest_digest: digest.clone(),
                    restore_duration_ms: 1_200,
                    migration_version_matched: true,
                    archive_table_count: Some(1),
                    restored_table_count: Some(1),
                    restored_row_count: Some(1),
                    passed: true,
                    reason: "fixture".to_owned(),
                    occurred_at: now,
                },
                now,
            )
            .await
            .expect("recording the passing rehearsal should succeed");

        let preview = store
            .preview_recovery_execution(
                &RecoveryExecutionPreviewRequest {
                    policy_id: recovery_policy.policy_id,
                    requested_by: format!("requester-{suffix}"),
                    tenant_id: format!("tenant-{suffix}"),
                    requesting_node_id: format!("node-for-requester-{suffix}"),
                    requesting_public_key_fingerprint: format!(
                        "fingerprint-for-requester-{suffix}"
                    ),
                    artifact_request_id: artifact_request.request_id,
                    manifest_digest: digest,
                    safety_snapshot_receipt_id: safety_receipt.receipt_id,
                    safety_snapshot_digest: safety_receipt
                        .manifest_digest
                        .expect("the safety fixture always records a digest"),
                    rehearsal_id: rehearsal.rehearsal_id,
                    confirmation_window: Duration::from_secs(900),
                    idempotency_key: format!("recovery-preview-{suffix}"),
                },
                now,
            )
            .await
            .expect("a valid preview should succeed");

        preview.request.request_id
    }

    /// Everything a confirmed, executable recovery-execution request needs:
    /// a previewed request (see [`previewed_request`]) plus a `Confirmed`
    /// authorization from a second, distinct signing key.
    async fn confirmed_request(store: &AdministrationStore, suffix: &str) -> String {
        let request_id = previewed_request(store, suffix).await;
        store
            .confirm_recovery_execution(
                &request_id,
                "confirming-signing-key",
                "confirming-node",
                &format!("distinct-fingerprint-{suffix}"),
                SystemTime::now(),
            )
            .await
            .expect("confirming with a distinct key should succeed");
        request_id
    }

    #[tokio::test]
    async fn execution_refuses_without_a_confirmed_authorization() {
        let Some(pool) = pool() else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let suffix = unique_id("recovery-execution-not-confirmed");
        let store = AdministrationStore::connect(&pool)
            .await
            .expect("the test database should accept administration store connections");
        // Deliberately never confirmed.
        let request_id = previewed_request(&store, &suffix).await;

        let config = dummy_config(true);
        let result = store
            .execute_recovery(&request_id, &config, SystemTime::now())
            .await;
        assert!(matches!(
            result,
            Err(AdministrationStoreError::RecoveryExecutionNotConfirmed)
        ));
    }

    #[tokio::test]
    async fn execution_records_a_refused_receipt_when_not_single_tenant_attested() {
        let Some(pool) = pool() else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let suffix = unique_id("recovery-execution-unattested");
        let store = AdministrationStore::connect(&pool)
            .await
            .expect("the test database should accept administration store connections");
        let request_id = confirmed_request(&store, &suffix).await;

        let config = dummy_config(false);
        let receipt = store
            .execute_recovery(&request_id, &config, SystemTime::now())
            .await
            .expect("an unattested deployment is a refused receipt, not an error");
        assert_eq!(receipt.outcome, RecoveryExecutionOutcome::Refused);
        assert!(receipt.reason.contains("single-tenant"));
    }

    #[tokio::test]
    async fn execution_records_a_refused_receipt_when_the_rehearsal_has_gone_stale() {
        let Some(pool) = pool() else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let suffix = unique_id("recovery-execution-stale-rehearsal");
        let store = AdministrationStore::connect(&pool)
            .await
            .expect("the test database should accept administration store connections");
        let request_id = confirmed_request(&store, &suffix).await;

        // A `now` far enough past the rehearsal's own `occurred_at` that the
        // freshness window has elapsed, without waiting 24 real hours.
        let far_future = SystemTime::now()
            + snapshot_provider::MAX_REHEARSAL_FRESHNESS
            + Duration::from_secs(3600);
        let config = dummy_config(true);
        let receipt = store
            .execute_recovery(&request_id, &config, far_future)
            .await
            .expect("a stale rehearsal is a refused receipt, not an error");
        assert_eq!(receipt.outcome, RecoveryExecutionOutcome::Refused);
        assert!(
            receipt.reason.to_lowercase().contains("rehearsal")
                || receipt.reason.to_lowercase().contains("freshness")
        );
    }

    #[tokio::test]
    async fn execution_is_idempotent_and_never_re_executes() {
        let Some(pool) = pool() else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let suffix = unique_id("recovery-execution-idempotent");
        let store = AdministrationStore::connect(&pool)
            .await
            .expect("the test database should accept administration store connections");
        let request_id = confirmed_request(&store, &suffix).await;

        // Unattested, so this refuses deterministically without needing a
        // real pg_restore -- exercising idempotency, not the restore path
        // itself (already covered in `snapshot_provider`'s own tests).
        let config = dummy_config(false);
        let now = SystemTime::now();
        let first = store
            .execute_recovery(&request_id, &config, now)
            .await
            .expect("the first execution attempt should record a receipt");
        let second = store
            .execute_recovery(&request_id, &config, now)
            .await
            .expect("replaying execution against an existing receipt should succeed");
        assert_eq!(first.receipt_id, second.receipt_id);
    }

    /// The full orchestration path, end to end, against a real `pg_restore`:
    /// an ephemeral database stands in for production, already migrated and
    /// already holding this test's own policy/request/confirmation rows when
    /// a genuine snapshot artifact of an *earlier* state of that same
    /// database is restored back over it via `AdministrationStore::execute_recovery`
    /// -- the exact shape a real production restore takes (schema already
    /// present, `--clean --if-exists` doing real work), never exercised by
    /// the other tests in this module, which all deliberately refuse before
    /// reaching `snapshot_provider::execute_recovery`.
    #[tokio::test]
    async fn execution_through_the_store_runs_a_real_restore_and_records_a_succeeded_receipt() {
        let Some(rehearsal_url) = crate::test_support::rehearsal_test_url() else {
            eprintln!("skipping: ACKPLANE_TEST_REHEARSAL_DATABASE_URL is not set");
            return;
        };
        if tokio::process::Command::new("pg_dump")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await
            .is_err()
            || tokio::process::Command::new("pg_restore")
                .arg("--version")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .await
                .is_err()
        {
            eprintln!("skipping: pg_dump/pg_restore is not available on PATH");
            return;
        }
        let suffix = unique_id("recovery-execution-real-restore");

        let rehearsal_url_for_config = rehearsal_url.clone();
        crate::test_support::with_ephemeral_database(
            &rehearsal_url,
            "ackplane_execute_recovery_store_target",
            |target_url| async move {
                let rehearsal_url = rehearsal_url_for_config;
                // Migrates the full Administration schema (through this
                // slice's own migration 61) into the stand-in database --
                // exactly the state a real deployment's `pg_restore --clean
                // --if-exists` target already has.
                let target_pool =
                    crate::db_pool::build_pool(&target_url, crate::db_pool::TEST_POOL_MAX_SIZE)
                        .expect("the ephemeral target database url should build a pool");
                let store = AdministrationStore::connect(&target_pool)
                    .await
                    .expect("the ephemeral target database should accept a store connection");

                let dir =
                    std::env::temp_dir().join(format!("ackplane-execute-recovery-store-{suffix}"));
                let _ = std::fs::remove_dir_all(&dir);
                let config = SnapshotProviderConfig {
                    database_url: target_url.clone(),
                    snapshot_dir: dir.clone(),
                    key_path: dir.join("key.bin"),
                    pg_dump_path: "pg_dump".to_string(),
                    pg_restore_path: "pg_restore".to_string(),
                    rehearsal_database_url: Some(rehearsal_url.clone()),
                    single_tenant_attested: true,
                };

                // The artifact: a real `pg_dump` of the target *before* this
                // test's own policy/request/confirmation rows exist, so a
                // successful restore genuinely reverts to a distinguishable
                // earlier state, never a no-op against identical content.
                let artifact = snapshot_provider::create_platform_snapshot(
                    &config,
                    &format!("execute-recovery-store-{suffix}"),
                )
                .await
                .expect("a real pg_dump against the freshly migrated target should succeed");

                let now = SystemTime::now();
                let snapshot_policy = store
                    .adopt_policy(&PolicyAdoptionRequest {
                        operation: AdministrationOperation::Snapshot,
                        scope: AdministrationScope::Platform,
                        data_classification: "operational-metadata".to_owned(),
                        retention_basis: "test fixture".to_owned(),
                        adopted_by: "loopback-principal".to_owned(),
                        idempotency_key: format!("snapshot-policy-{suffix}"),
                        effective_at: now,
                        expires_at: now + Duration::from_secs(3600),
                    })
                    .await
                    .expect("snapshot policy adoption should succeed")
                    .policy;
                let recovery_policy = store
                    .adopt_policy(&PolicyAdoptionRequest {
                        operation: AdministrationOperation::RecoveryExecution,
                        scope: AdministrationScope::Platform,
                        data_classification: "operational-metadata".to_owned(),
                        retention_basis: "test fixture".to_owned(),
                        adopted_by: "loopback-principal".to_owned(),
                        idempotency_key: format!("recovery-policy-{suffix}"),
                        effective_at: now,
                        expires_at: now + Duration::from_secs(3600),
                    })
                    .await
                    .expect("recovery policy adoption should succeed")
                    .policy;

                let artifact_request = store
                    .request_snapshot(
                        &NewSnapshotRequest {
                            policy_id: snapshot_policy.policy_id.clone(),
                            requested_by: "loopback-principal".to_owned(),
                            scope: AdministrationScope::Platform,
                            idempotency_key: format!("artifact-snapshot-{suffix}"),
                        },
                        now,
                    )
                    .await
                    .expect("the artifact snapshot request should succeed")
                    .request;
                store
                    .record_snapshot_receipt(
                        &NewSnapshotReceipt {
                            request_id: artifact_request.request_id.clone(),
                            outcome: SnapshotOutcome::Succeeded,
                            reason: "a real pg_dump completed".to_owned(),
                            artifact_path: Some(artifact.artifact_path.clone()),
                            manifest_digest: Some(artifact.manifest_digest.clone()),
                            encryption_key_id: Some(artifact.encryption_key_id.clone()),
                            size_bytes: Some(artifact.size_bytes),
                            verified: true,
                            occurred_at: now,
                        },
                        now,
                    )
                    .await
                    .expect("recording the artifact receipt should succeed");

                let safety_request = store
                    .request_snapshot(
                        &NewSnapshotRequest {
                            policy_id: snapshot_policy.policy_id,
                            requested_by: "loopback-principal".to_owned(),
                            scope: AdministrationScope::Platform,
                            idempotency_key: format!("safety-snapshot-{suffix}"),
                        },
                        now,
                    )
                    .await
                    .expect("the safety snapshot request should succeed")
                    .request;
                let safety_receipt = store
                    .record_snapshot_receipt(
                        &NewSnapshotReceipt {
                            request_id: safety_request.request_id,
                            outcome: SnapshotOutcome::Succeeded,
                            reason: "fixture".to_owned(),
                            artifact_path: Some(format!("/tmp/safety-{suffix}.snapshot")),
                            manifest_digest: Some(fixture_digest(9)),
                            encryption_key_id: Some("ackplane-snapshot-key-v1".to_owned()),
                            size_bytes: Some(4096),
                            verified: true,
                            occurred_at: now,
                        },
                        now,
                    )
                    .await
                    .expect("recording the safety receipt should succeed");

                let rehearsal = store
                    .record_recovery_rehearsal(
                        &NewRecoveryRehearsal {
                            request_id: artifact_request.request_id.clone(),
                            requested_by: "loopback-principal".to_owned(),
                            manifest_digest: artifact.manifest_digest.clone(),
                            restore_duration_ms: 1_200,
                            migration_version_matched: true,
                            archive_table_count: Some(1),
                            restored_table_count: Some(1),
                            restored_row_count: Some(1),
                            passed: true,
                            reason: "fixture".to_owned(),
                            occurred_at: now,
                        },
                        now,
                    )
                    .await
                    .expect("recording the passing rehearsal should succeed");

                let preview = store
                    .preview_recovery_execution(
                        &RecoveryExecutionPreviewRequest {
                            policy_id: recovery_policy.policy_id,
                            requested_by: format!("requester-{suffix}"),
                            tenant_id: format!("tenant-{suffix}"),
                            requesting_node_id: format!("node-for-requester-{suffix}"),
                            requesting_public_key_fingerprint: format!(
                                "fingerprint-for-requester-{suffix}"
                            ),
                            artifact_request_id: artifact_request.request_id,
                            manifest_digest: artifact.manifest_digest.clone(),
                            safety_snapshot_receipt_id: safety_receipt.receipt_id,
                            safety_snapshot_digest: safety_receipt
                                .manifest_digest
                                .expect("the safety fixture always records a digest"),
                            rehearsal_id: rehearsal.rehearsal_id,
                            confirmation_window: Duration::from_secs(900),
                            idempotency_key: format!("recovery-preview-{suffix}"),
                        },
                        now,
                    )
                    .await
                    .expect("a valid preview should succeed");

                store
                    .confirm_recovery_execution(
                        &preview.request.request_id,
                        "confirming-signing-key",
                        "confirming-node",
                        &format!("distinct-fingerprint-{suffix}"),
                        now,
                    )
                    .await
                    .expect("confirming with a distinct key should succeed");

                let receipt = store
                    .execute_recovery(&preview.request.request_id, &config, SystemTime::now())
                    .await
                    .expect("a genuine confirmed, fresh, attested execution should not error");
                assert_eq!(
                    receipt.outcome,
                    RecoveryExecutionOutcome::Succeeded,
                    "reason was {}",
                    receipt.reason
                );
                assert_eq!(receipt.new_manifest_digest, artifact.manifest_digest);

                let _ = std::fs::remove_dir_all(&dir);
            },
        )
        .await;
    }
}

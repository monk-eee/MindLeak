//! Transactional recovery-execution preview/confirmation persistence
//! (ADR-0145 decision 4-5), mirroring `purge_write.rs`'s preview/confirm
//! shape. Unlike Lifecycle purge, confirming here never mutates production:
//! it only records that a second, distinct enrolled key authorized this
//! exact request. Slice 4 is the only place `pg_restore` against
//! `ACKPLANE_DATABASE_URL` may run, gated on this confirmation existing.

use std::time::SystemTime;

use tokio_postgres::Transaction;

use super::recovery_execution_model::{
    assigned_confirmation_id, assigned_request_id, confirmation_digest, confirmation_from_row,
    preview_request_digest, request_from_row, validate_confirmation, validate_preview_request,
    NewRecoveryConfirmation, RecoveryConfirmation, RecoveryConfirmationOutcome,
    RecoveryExecutionPreviewRequest, RecoveryExecutionRequest, RecoveryExecutionRequestOutcome,
};
use super::{
    model::require_identifier, AdministrationOperation, AdministrationScope, AdministrationStore,
    AdministrationStoreError,
};

impl AdministrationStore {
    /// Computes and durably records a recovery-execution preview, refusing
    /// (before any request row exists) if no active `RecoveryExecution`
    /// policy authorizes this deployment, the named artifact has no
    /// succeeded Snapshot receipt (or the caller's declared digest does not
    /// match it), or the named rehearsal report does not exist, did not
    /// pass, or covers a different artifact digest -- or returns an
    /// identical prior preview's exact record. The safety Snapshot has
    /// already been captured by the caller (the Bridge route) before this is
    /// called; its failure fails the preview by never reaching this call.
    pub async fn preview_recovery_execution(
        &self,
        request: &RecoveryExecutionPreviewRequest,
        now: SystemTime,
    ) -> Result<RecoveryExecutionRequestOutcome, AdministrationStoreError> {
        validate_preview_request(request)?;
        let assigned_id = assigned_request_id(request);
        let digest = preview_request_digest(request)?;
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;

        if let Some(existing) = existing_request_by_id(&transaction, &assigned_id).await? {
            let outcome = replay_or_conflict(existing, &digest)?;
            transaction.commit().await?;
            return Ok(outcome);
        }

        let policy_row = transaction
            .query_opt(
                "SELECT * FROM administration_policies \
                 WHERE policy_id = $1 AND operation = $2 AND scope_kind = $3 \
                   AND tenant_id IS NOT DISTINCT FROM $4 \
                   AND revoked_at IS NULL AND effective_at <= $5 AND expires_at > $5",
                &[
                    &request.policy_id,
                    &AdministrationOperation::RecoveryExecution.as_i16(),
                    &AdministrationScope::Platform.kind_i16(),
                    &AdministrationScope::Platform.tenant_id(),
                    &now,
                ],
            )
            .await?;
        if policy_row.is_none() {
            transaction.commit().await?;
            return Err(AdministrationStoreError::NoActivePolicy);
        }

        let artifact_receipt = transaction
            .query_opt(
                "SELECT outcome, manifest_digest FROM administration_snapshot_receipts \
                 WHERE request_id = $1",
                &[&request.artifact_request_id],
            )
            .await?;
        match artifact_receipt {
            // 1 = SnapshotOutcome::Succeeded (see model::SnapshotOutcome).
            Some(row) if row.get::<_, i16>("outcome") == 1 => {
                let recorded_digest: Option<Vec<u8>> = row.get("manifest_digest");
                if recorded_digest.as_deref() != Some(request.manifest_digest.as_slice()) {
                    transaction.commit().await?;
                    return Err(AdministrationStoreError::RecoveryArtifactManifestMismatch);
                }
            }
            _ => {
                transaction.commit().await?;
                return Err(AdministrationStoreError::UnknownRecoveryArtifact);
            }
        }

        let rehearsal_row = transaction
            .query_opt(
                "SELECT 1 FROM administration_recovery_rehearsals \
                 WHERE rehearsal_id = $1 AND manifest_digest = $2 AND passed = true",
                &[&request.rehearsal_id, &request.manifest_digest],
            )
            .await?;
        if rehearsal_row.is_none() {
            transaction.commit().await?;
            return Err(AdministrationStoreError::NoPassingRehearsalForArtifact);
        }

        let confirmation_expires_at = now + request.confirmation_window;
        let inserted = transaction
            .query_opt(
                "INSERT INTO administration_recovery_execution_requests (request_id, \
                     policy_id, requested_by, tenant_id, requesting_node_id, \
                     requesting_public_key_fingerprint, artifact_request_id, manifest_digest, \
                     safety_snapshot_receipt_id, safety_snapshot_digest, rehearsal_id, \
                     confirmation_expires_at, idempotency_key, requested_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) \
                 ON CONFLICT DO NOTHING RETURNING *",
                &[
                    &assigned_id,
                    &request.policy_id,
                    &request.requested_by,
                    &request.tenant_id,
                    &request.requesting_node_id,
                    &request.requesting_public_key_fingerprint,
                    &request.artifact_request_id,
                    &request.manifest_digest,
                    &request.safety_snapshot_receipt_id,
                    &request.safety_snapshot_digest,
                    &request.rehearsal_id,
                    &confirmation_expires_at,
                    &request.idempotency_key,
                    &now,
                ],
            )
            .await?;
        let (recovery_request, idempotent_replay) = match inserted {
            Some(row) => (request_from_row(&row)?, false),
            None => {
                let existing = existing_request_by_id(&transaction, &assigned_id)
                    .await?
                    .ok_or(AdministrationStoreError::RequestIdempotencyConflict)?;
                let outcome = replay_or_conflict(existing, &digest)?;
                (outcome.request, true)
            }
        };
        transaction.commit().await?;
        Ok(RecoveryExecutionRequestOutcome {
            request: recovery_request,
            idempotent_replay,
        })
    }

    /// Authorizes (or refuses) a previously previewed recovery execution.
    /// Idempotent: a request that already has a confirmation returns it
    /// unchanged. The confirmer is a verified enrolled signing-key principal
    /// and must differ from the key that created the preview. This never
    /// executes `pg_restore`; it only records that authorization exists for
    /// slice 4 to later consume.
    pub async fn confirm_recovery_execution(
        &self,
        request_id: &str,
        confirming_signing_key_id: &str,
        confirming_node_id: &str,
        confirming_public_key_fingerprint: &str,
        now: SystemTime,
    ) -> Result<RecoveryConfirmation, AdministrationStoreError> {
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        let request = existing_request_by_id_for_update(&transaction, request_id)
            .await?
            .ok_or_else(
                || AdministrationStoreError::UnknownRecoveryExecutionRequest {
                    request_id: request_id.to_string(),
                },
            )?;
        if let Some(existing) = existing_confirmation_by_request(&transaction, request_id).await? {
            transaction.commit().await?;
            return Ok(existing);
        }
        require_identifier("confirming_signing_key_id", confirming_signing_key_id)?;
        require_identifier("confirming_node_id", confirming_node_id)?;
        require_identifier(
            "confirming_public_key_fingerprint",
            confirming_public_key_fingerprint,
        )?;
        if request.requesting_public_key_fingerprint == confirming_public_key_fingerprint {
            return Err(AdministrationStoreError::SelfConfirmationRefused);
        }

        let outcome = if request.confirmation_expires_at <= now {
            (
                RecoveryConfirmationOutcome::Expired,
                "The confirmation window elapsed; request a fresh preview.".to_string(),
            )
        } else {
            let policy_still_active = transaction
                .query_opt(
                    "SELECT 1 FROM administration_policies \
                     WHERE policy_id = $1 AND revoked_at IS NULL AND effective_at <= $2 AND expires_at > $2",
                    &[&request.policy_id, &now],
                )
                .await?
                .is_some();
            if !policy_still_active {
                (
                    RecoveryConfirmationOutcome::Refused,
                    "The authorizing policy was revoked or expired since the preview.".to_string(),
                )
            } else {
                (
                    RecoveryConfirmationOutcome::Confirmed,
                    "A second, distinct enrolled key authorized this request. Production \
                     execution is a separate, later step (ADR-0145 slice 4)."
                        .to_string(),
                )
            }
        };

        let new_confirmation = NewRecoveryConfirmation {
            request_id: request_id.to_string(),
            outcome: outcome.0,
            reason: outcome.1,
            occurred_at: now,
            confirming_signing_key_id: Some(confirming_signing_key_id.to_string()),
            confirming_node_id: Some(confirming_node_id.to_string()),
            confirming_public_key_fingerprint: Some(confirming_public_key_fingerprint.to_string()),
        };
        validate_confirmation(&new_confirmation, now)?;
        let digest = confirmation_digest(&new_confirmation)?;
        let assigned_id = assigned_confirmation_id(&new_confirmation);
        let inserted = transaction
            .query_opt(
                "INSERT INTO administration_recovery_execution_confirmations (confirmation_id, \
                     request_id, outcome, reason, occurred_at, recorded_at, \
                     confirming_signing_key_id, confirming_node_id, \
                     confirming_public_key_fingerprint) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) \
                 ON CONFLICT DO NOTHING RETURNING *",
                &[
                    &assigned_id,
                    &new_confirmation.request_id,
                    &new_confirmation.outcome.as_i16(),
                    &new_confirmation.reason,
                    &new_confirmation.occurred_at,
                    &now,
                    &new_confirmation.confirming_signing_key_id,
                    &new_confirmation.confirming_node_id,
                    &new_confirmation.confirming_public_key_fingerprint,
                ],
            )
            .await?;
        let confirmation = match inserted {
            Some(row) => confirmation_from_row(&row)?,
            None => {
                let existing = existing_confirmation_by_request(&transaction, request_id)
                    .await?
                    .ok_or(AdministrationStoreError::ReceiptConflict)?;
                if confirmation_digest(&NewRecoveryConfirmation {
                    request_id: existing.request_id.clone(),
                    outcome: existing.outcome,
                    reason: existing.reason.clone(),
                    occurred_at: existing.occurred_at,
                    confirming_signing_key_id: existing.confirming_signing_key_id.clone(),
                    confirming_node_id: existing.confirming_node_id.clone(),
                    confirming_public_key_fingerprint: existing
                        .confirming_public_key_fingerprint
                        .clone(),
                })? != digest
                {
                    return Err(AdministrationStoreError::ReceiptConflict);
                }
                existing
            }
        };
        transaction.commit().await?;
        Ok(confirmation)
    }

    pub async fn recovery_execution_request(
        &self,
        request_id: &str,
    ) -> Result<Option<RecoveryExecutionRequest>, AdministrationStoreError> {
        let connection = self.connection().await?;
        let row = connection
            .query_opt(
                "SELECT * FROM administration_recovery_execution_requests WHERE request_id = $1",
                &[&request_id],
            )
            .await?;
        row.as_ref().map(request_from_row).transpose()
    }

    pub async fn recovery_confirmation_for_request(
        &self,
        request_id: &str,
    ) -> Result<Option<RecoveryConfirmation>, AdministrationStoreError> {
        let connection = self.connection().await?;
        let row = connection
            .query_opt(
                "SELECT * FROM administration_recovery_execution_confirmations \
                 WHERE request_id = $1",
                &[&request_id],
            )
            .await?;
        row.as_ref().map(confirmation_from_row).transpose()
    }
}

async fn existing_request_by_id(
    transaction: &Transaction<'_>,
    request_id: &str,
) -> Result<Option<RecoveryExecutionRequest>, AdministrationStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM administration_recovery_execution_requests WHERE request_id = $1",
            &[&request_id],
        )
        .await?
        .as_ref()
        .map(request_from_row)
        .transpose()
}

async fn existing_request_by_id_for_update(
    transaction: &Transaction<'_>,
    request_id: &str,
) -> Result<Option<RecoveryExecutionRequest>, AdministrationStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM administration_recovery_execution_requests \
             WHERE request_id = $1 FOR UPDATE",
            &[&request_id],
        )
        .await?
        .as_ref()
        .map(request_from_row)
        .transpose()
}

fn replay_or_conflict(
    request: RecoveryExecutionRequest,
    digest: &[u8],
) -> Result<RecoveryExecutionRequestOutcome, AdministrationStoreError> {
    let recomputed = preview_request_digest(&RecoveryExecutionPreviewRequest {
        policy_id: request.policy_id.clone(),
        requested_by: request.requested_by.clone(),
        tenant_id: request.tenant_id.clone(),
        requesting_node_id: request.requesting_node_id.clone(),
        requesting_public_key_fingerprint: request.requesting_public_key_fingerprint.clone(),
        artifact_request_id: request.artifact_request_id.clone(),
        manifest_digest: request.manifest_digest.clone(),
        safety_snapshot_receipt_id: request.safety_snapshot_receipt_id.clone(),
        safety_snapshot_digest: request.safety_snapshot_digest.clone(),
        rehearsal_id: request.rehearsal_id.clone(),
        // Not part of the identity digest (see `preview_request_digest`);
        // any positive placeholder recomputes the same bytes.
        confirmation_window: std::time::Duration::from_secs(1),
        idempotency_key: request.idempotency_key.clone(),
    })?;
    if recomputed == digest {
        Ok(RecoveryExecutionRequestOutcome {
            request,
            idempotent_replay: true,
        })
    } else {
        Err(AdministrationStoreError::RequestIdempotencyConflict)
    }
}

async fn existing_confirmation_by_request(
    transaction: &Transaction<'_>,
    request_id: &str,
) -> Result<Option<RecoveryConfirmation>, AdministrationStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM administration_recovery_execution_confirmations \
             WHERE request_id = $1",
            &[&request_id],
        )
        .await?
        .as_ref()
        .map(confirmation_from_row)
        .transpose()
}

//! Transactional Lifecycle-purge preview/confirm persistence (ADR-0119
//! decisions 1, 7, 9).
//!
//! Preview computes and records a read-only impact count; confirm is the
//! only place that deletes rows, and only for the exact scope and cutoff the
//! preview already named. Neither step ever issues `DROP DATABASE`,
//! `TRUNCATE`, or a schema-wide statement -- both run a single scoped,
//! parameterized query against the one closed data category's own table.

use std::time::SystemTime;

use tokio_postgres::Transaction;

use super::purge_model::{
    assigned_receipt_id, assigned_request_id, preview_request_digest, receipt_digest,
    receipt_from_row, request_from_row, validate_preview_request, validate_receipt,
    NewPurgeReceipt, PurgeDataCategory, PurgeOutcome, PurgePreviewRequest, PurgeReceipt,
    PurgeRequest, PurgeRequestOutcome,
};
use super::{
    model::require_identifier, AdministrationOperation, AdministrationStore,
    AdministrationStoreError,
};

impl AdministrationStore {
    /// Computes and durably records a purge impact preview, refusing
    /// (before any request row exists) if no active `LifecyclePurge` policy
    /// authorizes this tenant/repository, or returns an identical prior
    /// preview's exact record.
    pub async fn preview_purge(
        &self,
        request: &PurgePreviewRequest,
        now: SystemTime,
    ) -> Result<PurgeRequestOutcome, AdministrationStoreError> {
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

        async fn existing_request_by_id_for_update(
            transaction: &Transaction<'_>,
            request_id: &str,
        ) -> Result<Option<PurgeRequest>, AdministrationStoreError> {
            transaction
                .query_opt(
                    "SELECT * FROM administration_purge_requests WHERE request_id = $1 FOR UPDATE",
                    &[&request_id],
                )
                .await?
                .as_ref()
                .map(request_from_row)
                .transpose()
        }

        let policy_row = transaction
            .query_opt(
                "SELECT * FROM administration_policies \
                 WHERE policy_id = $1 AND operation = $2 AND scope_kind = 2 \
                   AND tenant_id = $3 \
                   AND revoked_at IS NULL AND effective_at <= $4 AND expires_at > $4",
                &[
                    &request.policy_id,
                    &AdministrationOperation::LifecyclePurge.as_i16(),
                    &request.tenant_id,
                    &now,
                ],
            )
            .await?;
        if policy_row.is_none() {
            transaction.commit().await?;
            return Err(AdministrationStoreError::NoActivePolicy);
        }

        let preview_row_count = count_purge_candidates(
            &transaction,
            request.data_category,
            &request.tenant_id,
            &request.repository_id,
            request.older_than,
        )
        .await?;
        let confirmation_expires_at = now + request.confirmation_window;

        let inserted = transaction
            .query_opt(
                "INSERT INTO administration_purge_requests (request_id, policy_id, \
                     requested_by, requesting_node_id, tenant_id, repository_id, data_category, \
                     requesting_public_key_fingerprint, older_than, preview_row_count, \
                     confirmation_expires_at, idempotency_key, requested_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) \
                 ON CONFLICT DO NOTHING RETURNING *",
                &[
                    &assigned_id,
                    &request.policy_id,
                    &request.requested_by,
                    &request.requesting_node_id,
                    &request.tenant_id,
                    &request.repository_id,
                    &request.data_category.as_i16(),
                    &request.requesting_public_key_fingerprint,
                    &request.older_than,
                    &preview_row_count,
                    &confirmation_expires_at,
                    &request.idempotency_key,
                    &now,
                ],
            )
            .await?;
        let (purge_request, idempotent_replay) = match inserted {
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
        Ok(PurgeRequestOutcome {
            request: purge_request,
            idempotent_replay,
        })
    }

    /// Executes (or refuses) a previously previewed purge. Idempotent: a
    /// request that already has a receipt returns it unchanged rather than
    /// deleting a second time. The confirmer is a verified enrolled
    /// signing-key principal and must differ from the key that created the
    /// preview; caller-provided labels never authorize a purge.
    pub async fn confirm_purge(
        &self,
        request_id: &str,
        confirming_signing_key_id: &str,
        confirming_node_id: &str,
        confirming_public_key_fingerprint: &str,
        now: SystemTime,
    ) -> Result<PurgeReceipt, AdministrationStoreError> {
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        let request = existing_request_by_id_for_update(&transaction, request_id)
            .await?
            .ok_or_else(|| AdministrationStoreError::UnknownPurgeRequest {
                request_id: request_id.to_string(),
            })?;
        if let Some(existing) = existing_receipt_by_request(&transaction, request_id).await? {
            transaction.commit().await?;
            return Ok(existing);
        }
        if request.requesting_node_id.is_none() {
            return Err(AdministrationStoreError::LegacyPurgeRequestUnauthenticated);
        }
        require_identifier("confirming_signing_key_id", confirming_signing_key_id)?;
        require_identifier("confirming_node_id", confirming_node_id)?;
        require_identifier(
            "confirming_public_key_fingerprint",
            confirming_public_key_fingerprint,
        )?;
        let requesting_fingerprint = request
            .requesting_public_key_fingerprint
            .as_deref()
            .ok_or(AdministrationStoreError::LegacyPurgeRequestUnauthenticated)?;
        if requesting_fingerprint == confirming_public_key_fingerprint {
            return Err(AdministrationStoreError::SelfConfirmationRefused);
        }

        let outcome = if request.confirmation_expires_at <= now {
            (
                PurgeOutcome::Expired,
                "The confirmation window elapsed; request a fresh preview.".to_string(),
                None,
                Some(confirming_signing_key_id.to_string()),
                Some(confirming_node_id.to_string()),
                Some(confirming_public_key_fingerprint.to_string()),
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
                    PurgeOutcome::Refused,
                    "The authorizing policy was revoked or expired since the preview.".to_string(),
                    None,
                    Some(confirming_signing_key_id.to_string()),
                    Some(confirming_node_id.to_string()),
                    Some(confirming_public_key_fingerprint.to_string()),
                )
            } else {
                let deleted = delete_purge_candidates(
                    &transaction,
                    request.data_category,
                    &request.tenant_id,
                    &request.repository_id,
                    request.older_than,
                )
                .await?;
                (
                    PurgeOutcome::Succeeded,
                    format!("Deleted {deleted} row(s) matching the previewed scope and cutoff."),
                    Some(deleted),
                    Some(confirming_signing_key_id.to_string()),
                    Some(confirming_node_id.to_string()),
                    Some(confirming_public_key_fingerprint.to_string()),
                )
            }
        };

        let new_receipt = NewPurgeReceipt {
            request_id: request_id.to_string(),
            outcome: outcome.0,
            reason: outcome.1,
            rows_deleted: outcome.2,
            occurred_at: now,
            confirming_signing_key_id: outcome.3,
            confirming_node_id: outcome.4,
            confirming_public_key_fingerprint: outcome.5,
        };
        validate_receipt(&new_receipt, now)?;
        let digest = receipt_digest(&new_receipt)?;
        let assigned_id = assigned_receipt_id(&new_receipt);
        let inserted = transaction
            .query_opt(
                "INSERT INTO administration_purge_receipts (receipt_id, request_id, outcome, \
                     reason, rows_deleted, occurred_at, recorded_at, \
                     confirming_signing_key_id, confirming_node_id, \
                     confirming_public_key_fingerprint) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
                 ON CONFLICT DO NOTHING RETURNING *",
                &[
                    &assigned_id,
                    &new_receipt.request_id,
                    &new_receipt.outcome.as_i16(),
                    &new_receipt.reason,
                    &new_receipt.rows_deleted,
                    &new_receipt.occurred_at,
                    &now,
                    &new_receipt.confirming_signing_key_id,
                    &new_receipt.confirming_node_id,
                    &new_receipt.confirming_public_key_fingerprint,
                ],
            )
            .await?;
        let receipt = match inserted {
            Some(row) => receipt_from_row(&row)?,
            None => {
                let existing = existing_receipt_by_request(&transaction, request_id)
                    .await?
                    .ok_or(AdministrationStoreError::ReceiptConflict)?;
                if receipt_digest(&NewPurgeReceipt {
                    request_id: existing.request_id.clone(),
                    outcome: existing.outcome,
                    reason: existing.reason.clone(),
                    rows_deleted: existing.rows_deleted,
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
        Ok(receipt)
    }

    pub async fn purge_request(
        &self,
        request_id: &str,
    ) -> Result<Option<PurgeRequest>, AdministrationStoreError> {
        let connection = self.connection().await?;
        let row = connection
            .query_opt(
                "SELECT * FROM administration_purge_requests WHERE request_id = $1",
                &[&request_id],
            )
            .await?;
        row.as_ref().map(request_from_row).transpose()
    }

    pub async fn purge_receipt_for_request(
        &self,
        request_id: &str,
    ) -> Result<Option<PurgeReceipt>, AdministrationStoreError> {
        let connection = self.connection().await?;
        let row = connection
            .query_opt(
                "SELECT * FROM administration_purge_receipts WHERE request_id = $1",
                &[&request_id],
            )
            .await?;
        row.as_ref().map(receipt_from_row).transpose()
    }
}

async fn existing_request_by_id(
    transaction: &Transaction<'_>,
    request_id: &str,
) -> Result<Option<PurgeRequest>, AdministrationStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM administration_purge_requests WHERE request_id = $1",
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
) -> Result<Option<PurgeRequest>, AdministrationStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM administration_purge_requests WHERE request_id = $1 FOR UPDATE",
            &[&request_id],
        )
        .await?
        .as_ref()
        .map(request_from_row)
        .transpose()
}

fn replay_or_conflict(
    request: PurgeRequest,
    digest: &[u8],
) -> Result<PurgeRequestOutcome, AdministrationStoreError> {
    let recomputed = preview_request_digest(&PurgePreviewRequest {
        policy_id: request.policy_id.clone(),
        requested_by: request.requested_by.clone(),
        requesting_node_id: request.requesting_node_id.clone().unwrap_or_default(),
        requesting_public_key_fingerprint: request
            .requesting_public_key_fingerprint
            .clone()
            .unwrap_or_default(),
        tenant_id: request.tenant_id.clone(),
        repository_id: request.repository_id.clone(),
        data_category: request.data_category,
        older_than: request.older_than,
        // Not part of the identity digest (see `preview_request_digest`);
        // any positive placeholder recomputes the same bytes.
        confirmation_window: std::time::Duration::from_secs(1),
        idempotency_key: request.idempotency_key.clone(),
    })?;
    if recomputed == digest {
        Ok(PurgeRequestOutcome {
            request,
            idempotent_replay: true,
        })
    } else {
        Err(AdministrationStoreError::RequestIdempotencyConflict)
    }
}

async fn existing_receipt_by_request(
    transaction: &Transaction<'_>,
    request_id: &str,
) -> Result<Option<PurgeReceipt>, AdministrationStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM administration_purge_receipts WHERE request_id = $1",
            &[&request_id],
        )
        .await?
        .as_ref()
        .map(receipt_from_row)
        .transpose()
}

/// The one closed switch a new `PurgeDataCategory` variant must extend --
/// count and delete stay in the same match so they can never drift apart
/// (a category counted here but deleted from a different table, or the
/// reverse).
async fn count_purge_candidates(
    transaction: &Transaction<'_>,
    category: PurgeDataCategory,
    tenant_id: &str,
    repository_id: &str,
    older_than: SystemTime,
) -> Result<i64, AdministrationStoreError> {
    match category {
        PurgeDataCategory::TelemetryEvents => {
            let row = transaction
                .query_one(
                    "SELECT COUNT(*) FROM telemetry_events \
                     WHERE tenant_id = $1 AND repository_id = $2 AND occurred_at < $3",
                    &[&tenant_id, &repository_id, &older_than],
                )
                .await?;
            Ok(row.get::<_, i64>(0))
        }
    }
}

async fn delete_purge_candidates(
    transaction: &Transaction<'_>,
    category: PurgeDataCategory,
    tenant_id: &str,
    repository_id: &str,
    older_than: SystemTime,
) -> Result<i64, AdministrationStoreError> {
    match category {
        PurgeDataCategory::TelemetryEvents => {
            let deleted = transaction
                .execute(
                    "DELETE FROM telemetry_events \
                     WHERE tenant_id = $1 AND repository_id = $2 AND occurred_at < $3",
                    &[&tenant_id, &repository_id, &older_than],
                )
                .await?;
            Ok(i64::try_from(deleted).unwrap_or(i64::MAX))
        }
    }
}

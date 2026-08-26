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
    receipt_from_row, request_from_row, validate_confirming_label, validate_preview_request,
    validate_receipt, NewPurgeReceipt, PurgeDataCategory, PurgeOutcome, PurgePreviewRequest,
    PurgeReceipt, PurgeRequest, PurgeRequestOutcome,
};
use super::{AdministrationOperation, AdministrationStore, AdministrationStoreError};

impl AdministrationStore {
    /// Computes and durably records a purge impact preview, refusing
    /// (before any request row exists) if no active `LifecyclePurge` policy
    /// authorizes this tenant/repository, or returns an identical prior
    /// preview's exact record.
    pub async fn preview_purge(
        &mut self,
        request: &PurgePreviewRequest,
        now: SystemTime,
    ) -> Result<PurgeRequestOutcome, AdministrationStoreError> {
        validate_preview_request(request)?;
        let assigned_id = assigned_request_id(request);
        let digest = preview_request_digest(request)?;
        let transaction = self.client.transaction().await?;

        if let Some(existing) = existing_request_by_id(&transaction, &assigned_id).await? {
            let outcome = replay_or_conflict(existing, &digest)?;
            transaction.commit().await?;
            return Ok(outcome);
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
                     requested_by, tenant_id, repository_id, data_category, older_than, \
                     preview_row_count, confirmation_expires_at, idempotency_key, requested_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
                 ON CONFLICT DO NOTHING RETURNING *",
                &[
                    &assigned_id,
                    &request.policy_id,
                    &request.requested_by,
                    &request.tenant_id,
                    &request.repository_id,
                    &request.data_category.as_i16(),
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
    /// deleting a second time. `confirming_label` must be non-empty and
    /// differ from the request's `requested_by` (ADR-0119 decision 7): a
    /// same-label or empty-label attempt is refused before any receipt is
    /// written, since `administration_purge_receipts` allows only one
    /// receipt per request ever and a caller must be able to retry with a
    /// correct label inside the same confirmation window.
    pub async fn confirm_purge(
        &mut self,
        request_id: &str,
        confirming_label: &str,
        now: SystemTime,
    ) -> Result<PurgeReceipt, AdministrationStoreError> {
        let transaction = self.client.transaction().await?;
        let request = existing_request_by_id(&transaction, request_id)
            .await?
            .ok_or_else(|| AdministrationStoreError::UnknownPurgeRequest {
                request_id: request_id.to_string(),
            })?;
        if let Some(existing) = existing_receipt_by_request(&transaction, request_id).await? {
            transaction.commit().await?;
            return Ok(existing);
        }

        let outcome = if request.confirmation_expires_at <= now {
            (
                PurgeOutcome::Expired,
                "The confirmation window elapsed; request a fresh preview.".to_string(),
                None,
                None,
            )
        } else {
            let confirming_label =
                validate_confirming_label(confirming_label, &request.requested_by)?;
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
                    Some(confirming_label),
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
                    Some(confirming_label),
                )
            }
        };

        let new_receipt = NewPurgeReceipt {
            request_id: request_id.to_string(),
            outcome: outcome.0,
            reason: outcome.1,
            rows_deleted: outcome.2,
            occurred_at: now,
            confirming_label: outcome.3,
        };
        validate_receipt(&new_receipt, now)?;
        let digest = receipt_digest(&new_receipt)?;
        let assigned_id = assigned_receipt_id(&new_receipt);
        let inserted = transaction
            .query_opt(
                "INSERT INTO administration_purge_receipts (receipt_id, request_id, outcome, \
                     reason, rows_deleted, occurred_at, recorded_at, confirming_label) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8) \
                 ON CONFLICT DO NOTHING RETURNING *",
                &[
                    &assigned_id,
                    &new_receipt.request_id,
                    &new_receipt.outcome.as_i16(),
                    &new_receipt.reason,
                    &new_receipt.rows_deleted,
                    &new_receipt.occurred_at,
                    &now,
                    &new_receipt.confirming_label,
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
                    confirming_label: existing.confirming_label.clone(),
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
        &mut self,
        request_id: &str,
    ) -> Result<Option<PurgeRequest>, AdministrationStoreError> {
        let row = self
            .client
            .query_opt(
                "SELECT * FROM administration_purge_requests WHERE request_id = $1",
                &[&request_id],
            )
            .await?;
        row.as_ref().map(request_from_row).transpose()
    }

    pub async fn purge_receipt_for_request(
        &mut self,
        request_id: &str,
    ) -> Result<Option<PurgeReceipt>, AdministrationStoreError> {
        let row = self
            .client
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

fn replay_or_conflict(
    request: PurgeRequest,
    digest: &[u8],
) -> Result<PurgeRequestOutcome, AdministrationStoreError> {
    let recomputed = preview_request_digest(&PurgePreviewRequest {
        policy_id: request.policy_id.clone(),
        requested_by: request.requested_by.clone(),
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

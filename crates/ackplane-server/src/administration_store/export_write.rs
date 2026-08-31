//! Transactional Export request/receipt persistence (ADR-0119 decision 5).
//!
//! Mirrors the Snapshot request/receipt flow in `write.rs` exactly (a single
//! request-then-receipt workflow, not Purge's two-phase preview/confirm --
//! decision 5 does not ask for a separate confirmation step the way decision
//! 9's destructive Lifecycle purge does). This store never builds the export
//! artifact itself: [`crate::export_provider`] is the one place that queries
//! the bounded, redacted representation and writes it to disk.

use std::time::SystemTime;

use tokio_postgres::Transaction;

use super::export_model::{
    assigned_receipt_id, assigned_request_id, receipt_digest, receipt_from_row, request_digest,
    request_from_row, validate_receipt, validate_request, ExportReceipt, ExportRequest,
    ExportRequestOutcome, NewExportReceipt, NewExportRequest,
};
use super::{AdministrationOperation, AdministrationStore, AdministrationStoreError};

impl AdministrationStore {
    /// Records an Export request, refusing (before any request row exists)
    /// without an active, tenant-scoped `Export` policy, or returns an
    /// identical prior request's exact record.
    pub async fn request_export(
        &self,
        request: &NewExportRequest,
        now: SystemTime,
    ) -> Result<ExportRequestOutcome, AdministrationStoreError> {
        validate_request(request)?;
        let assigned_id = assigned_request_id(request);
        let digest = request_digest(request)?;
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
                 WHERE policy_id = $1 AND operation = $2 AND scope_kind = 2 \
                   AND tenant_id = $3 \
                   AND revoked_at IS NULL AND effective_at <= $4 AND expires_at > $4",
                &[
                    &request.policy_id,
                    &AdministrationOperation::Export.as_i16(),
                    &request.tenant_id,
                    &now,
                ],
            )
            .await?;
        if policy_row.is_none() {
            transaction.commit().await?;
            return Err(AdministrationStoreError::NoActivePolicy);
        }

        let max_records = i32::try_from(request.max_records).unwrap_or(i32::MAX);
        let inserted = transaction
            .query_opt(
                "INSERT INTO administration_export_requests (request_id, policy_id, \
                     requested_by, tenant_id, repository_id, data_category, purpose, \
                     max_records, idempotency_key, requested_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
                 ON CONFLICT DO NOTHING RETURNING *",
                &[
                    &assigned_id,
                    &request.policy_id,
                    &request.requested_by,
                    &request.tenant_id,
                    &request.repository_id,
                    &request.data_category.as_i16(),
                    &request.purpose,
                    &max_records,
                    &request.idempotency_key,
                    &now,
                ],
            )
            .await?;
        let (export_request, idempotent_replay) = match inserted {
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
        Ok(ExportRequestOutcome {
            request: export_request,
            idempotent_replay,
        })
    }

    /// Appends one immutable Export receipt or returns its exact prior
    /// retry.
    pub async fn record_export_receipt(
        &self,
        receipt: &NewExportReceipt,
        now: SystemTime,
    ) -> Result<ExportReceipt, AdministrationStoreError> {
        validate_receipt(receipt, now)?;
        let digest = receipt_digest(receipt)?;
        let assigned_id = assigned_receipt_id(receipt);
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;

        ensure_request_exists(&transaction, &receipt.request_id).await?;
        if let Some(existing) = existing_receipt_by_id(&transaction, &assigned_id).await? {
            let receipt = replay_receipt_or_conflict(existing, &digest)?;
            transaction.commit().await?;
            return Ok(receipt);
        }

        let inserted = transaction
            .query_opt(
                "INSERT INTO administration_export_receipts (receipt_id, request_id, outcome, \
                     reason, artifact_path, manifest_digest, schema_version, record_count, \
                     redacted_fields, occurred_at, recorded_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
                 ON CONFLICT DO NOTHING RETURNING *",
                &[
                    &assigned_id,
                    &receipt.request_id,
                    &receipt.outcome.as_i16(),
                    &receipt.reason,
                    &receipt.artifact_path,
                    &receipt.manifest_digest,
                    &receipt.schema_version,
                    &receipt.record_count,
                    &receipt.redacted_fields,
                    &receipt.occurred_at,
                    &now,
                ],
            )
            .await?;
        let receipt = match inserted {
            Some(row) => receipt_from_row(&row)?,
            None => {
                let existing = existing_receipt_by_id(&transaction, &assigned_id)
                    .await?
                    .ok_or(AdministrationStoreError::ReceiptConflict)?;
                replay_receipt_or_conflict(existing, &digest)?
            }
        };
        transaction.commit().await?;
        Ok(receipt)
    }

    /// The request itself, so a caller can check `requested_by`/
    /// `repository_id` before disclosing its receipt to a different tenant.
    pub async fn export_request(
        &self,
        request_id: &str,
    ) -> Result<Option<ExportRequest>, AdministrationStoreError> {
        let connection = self.connection().await?;
        let row = connection
            .query_opt(
                "SELECT * FROM administration_export_requests WHERE request_id = $1",
                &[&request_id],
            )
            .await?;
        row.as_ref().map(request_from_row).transpose()
    }

    pub async fn export_receipt_for_request(
        &self,
        request_id: &str,
    ) -> Result<Option<ExportReceipt>, AdministrationStoreError> {
        let connection = self.connection().await?;
        let row = connection
            .query_opt(
                "SELECT * FROM administration_export_receipts WHERE request_id = $1",
                &[&request_id],
            )
            .await?;
        row.as_ref().map(receipt_from_row).transpose()
    }
}

async fn existing_request_by_id(
    transaction: &Transaction<'_>,
    request_id: &str,
) -> Result<Option<ExportRequest>, AdministrationStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM administration_export_requests WHERE request_id = $1",
            &[&request_id],
        )
        .await?
        .as_ref()
        .map(request_from_row)
        .transpose()
}

fn replay_or_conflict(
    request: ExportRequest,
    digest: &[u8],
) -> Result<ExportRequestOutcome, AdministrationStoreError> {
    let recomputed = request_digest(&NewExportRequest {
        policy_id: request.policy_id.clone(),
        requested_by: request.requested_by.clone(),
        tenant_id: request.tenant_id.clone(),
        repository_id: request.repository_id.clone(),
        data_category: request.data_category,
        purpose: request.purpose.clone(),
        max_records: request.max_records,
        idempotency_key: request.idempotency_key.clone(),
    })?;
    if recomputed == digest {
        Ok(ExportRequestOutcome {
            request,
            idempotent_replay: true,
        })
    } else {
        Err(AdministrationStoreError::RequestIdempotencyConflict)
    }
}

async fn ensure_request_exists(
    transaction: &Transaction<'_>,
    request_id: &str,
) -> Result<(), AdministrationStoreError> {
    existing_request_by_id(transaction, request_id)
        .await?
        .map(|_| ())
        .ok_or_else(|| AdministrationStoreError::UnknownExportRequest {
            request_id: request_id.to_string(),
        })
}

async fn existing_receipt_by_id(
    transaction: &Transaction<'_>,
    receipt_id: &str,
) -> Result<Option<ExportReceipt>, AdministrationStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM administration_export_receipts WHERE receipt_id = $1",
            &[&receipt_id],
        )
        .await?
        .as_ref()
        .map(receipt_from_row)
        .transpose()
}

fn replay_receipt_or_conflict(
    receipt: ExportReceipt,
    digest: &[u8],
) -> Result<ExportReceipt, AdministrationStoreError> {
    let recomputed = receipt_digest(&NewExportReceipt {
        request_id: receipt.request_id.clone(),
        outcome: receipt.outcome,
        reason: receipt.reason.clone(),
        artifact_path: receipt.artifact_path.clone(),
        manifest_digest: receipt.manifest_digest.clone(),
        schema_version: receipt.schema_version.clone(),
        record_count: receipt.record_count,
        redacted_fields: receipt.redacted_fields.clone(),
        occurred_at: receipt.occurred_at,
    })?;
    if recomputed == digest {
        Ok(receipt)
    } else {
        Err(AdministrationStoreError::ReceiptConflict)
    }
}

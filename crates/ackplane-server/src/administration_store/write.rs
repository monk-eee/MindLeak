//! Transactional adopted-policy and Snapshot request/receipt persistence.

use std::time::SystemTime;

use tokio_postgres::Transaction;

use super::{
    model::{
        assigned_policy_id, assigned_receipt_id, assigned_request_id, policy_digest,
        policy_from_row, snapshot_receipt_digest, snapshot_receipt_from_row,
        snapshot_request_digest, snapshot_request_from_row, validate_policy_request,
        validate_snapshot_receipt, validate_snapshot_request, AdministrationPolicy,
        AdministrationStoreError, NewSnapshotReceipt, NewSnapshotRequest, PolicyAdoptionRequest,
        PolicyWriteOutcome, SnapshotReceipt, SnapshotRequest, SnapshotRequestOutcome,
    },
    AdministrationOperation, AdministrationScope, AdministrationStore,
};

impl AdministrationStore {
    /// Adopts a new administration policy or returns its exact prior retry
    /// (ADR-0119 decision 2, ADR-0128 decision 2: the policy half of the
    /// authorization basis a verified principal alone never satisfies).
    pub async fn adopt_policy(
        &mut self,
        request: &PolicyAdoptionRequest,
    ) -> Result<PolicyWriteOutcome, AdministrationStoreError> {
        validate_policy_request(request)?;
        let assigned_id = assigned_policy_id(request);
        let digest = policy_digest(request)?;
        let transaction = self.client.transaction().await?;

        if let Some(policy) = existing_policy_by_id(&transaction, &assigned_id).await? {
            let outcome = replay_policy_or_conflict(policy, &digest)?;
            transaction.commit().await?;
            return Ok(outcome);
        }

        let inserted = transaction
            .query_opt(
                "INSERT INTO administration_policies (policy_id, operation, scope_kind, \
                     tenant_id, data_classification, retention_basis, adopted_by, \
                     idempotency_key, effective_at, expires_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
                 ON CONFLICT DO NOTHING RETURNING *",
                &[
                    &assigned_id,
                    &request.operation.as_i16(),
                    &request.scope.kind_i16(),
                    &request.scope.tenant_id(),
                    &request.data_classification,
                    &request.retention_basis,
                    &request.adopted_by,
                    &request.idempotency_key,
                    &request.effective_at,
                    &request.expires_at,
                ],
            )
            .await?;
        let (policy, idempotent_replay) = match inserted {
            Some(row) => (policy_from_row(&row)?, false),
            None => {
                let policy = existing_policy_by_id(&transaction, &assigned_id)
                    .await?
                    .ok_or(AdministrationStoreError::PolicyIdempotencyConflict)?;
                let outcome = replay_policy_or_conflict(policy, &digest)?;
                (outcome.policy, true)
            }
        };
        transaction.commit().await?;
        Ok(PolicyWriteOutcome {
            policy,
            idempotent_replay,
        })
    }

    /// The most recently adopted, still-active (unexpired, unrevoked) policy
    /// covering `operation`/`scope`, if any -- what `request_snapshot` (and
    /// every future privileged operation) must find before it does anything
    /// else (ADR-0119 decision 2).
    pub async fn active_policy(
        &mut self,
        operation: AdministrationOperation,
        scope: &AdministrationScope,
        now: SystemTime,
    ) -> Result<Option<AdministrationPolicy>, AdministrationStoreError> {
        let row = self
            .client
            .query_opt(
                "SELECT * FROM administration_policies \
                 WHERE operation = $1 AND scope_kind = $2 \
                   AND tenant_id IS NOT DISTINCT FROM $3 \
                   AND revoked_at IS NULL AND effective_at <= $4 AND expires_at > $4 \
                 ORDER BY recorded_at DESC LIMIT 1",
                &[
                    &operation.as_i16(),
                    &scope.kind_i16(),
                    &scope.tenant_id(),
                    &now,
                ],
            )
            .await?;
        row.as_ref().map(policy_from_row).transpose()
    }

    /// Records a Snapshot request, refusing one with no active policy
    /// (ADR-0119 decision 2), or returns its exact prior retry.
    pub async fn request_snapshot(
        &mut self,
        request: &NewSnapshotRequest,
        now: SystemTime,
    ) -> Result<SnapshotRequestOutcome, AdministrationStoreError> {
        validate_snapshot_request(request)?;
        let assigned_id = assigned_request_id(request);
        let digest = snapshot_request_digest(request)?;
        let transaction = self.client.transaction().await?;

        if let Some(existing) = existing_request_by_id(&transaction, &assigned_id).await? {
            let outcome = replay_request_or_conflict(existing, &digest)?;
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
                    &AdministrationOperation::Snapshot.as_i16(),
                    &request.scope.kind_i16(),
                    &request.scope.tenant_id(),
                    &now,
                ],
            )
            .await?;
        if policy_row.is_none() {
            transaction.commit().await?;
            return Err(AdministrationStoreError::NoActivePolicy);
        }

        let inserted = transaction
            .query_opt(
                "INSERT INTO administration_snapshot_requests (request_id, policy_id, \
                     requested_by, scope_kind, tenant_id, idempotency_key, requested_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7) \
                 ON CONFLICT DO NOTHING RETURNING *",
                &[
                    &assigned_id,
                    &request.policy_id,
                    &request.requested_by,
                    &request.scope.kind_i16(),
                    &request.scope.tenant_id(),
                    &request.idempotency_key,
                    &now,
                ],
            )
            .await?;
        let (snapshot_request, idempotent_replay) = match inserted {
            Some(row) => (snapshot_request_from_row(&row)?, false),
            None => {
                let existing = existing_request_by_id(&transaction, &assigned_id)
                    .await?
                    .ok_or(AdministrationStoreError::RequestIdempotencyConflict)?;
                let outcome = replay_request_or_conflict(existing, &digest)?;
                (outcome.request, true)
            }
        };
        transaction.commit().await?;
        Ok(SnapshotRequestOutcome {
            request: snapshot_request,
            idempotent_replay,
        })
    }

    /// Appends one immutable Snapshot receipt or returns its exact prior
    /// retry. The caller (the Snapshot provider) has already executed or
    /// refused the underlying operation; this only durably records the
    /// outcome (ADR-0119 decision 3: a web response never claims completion
    /// without a receipt).
    pub async fn record_snapshot_receipt(
        &mut self,
        receipt: &NewSnapshotReceipt,
        now: SystemTime,
    ) -> Result<SnapshotReceipt, AdministrationStoreError> {
        validate_snapshot_receipt(receipt, now)?;
        let digest = snapshot_receipt_digest(receipt)?;
        let assigned_id = assigned_receipt_id(receipt);
        let transaction = self.client.transaction().await?;

        ensure_request_exists(&transaction, &receipt.request_id).await?;
        if let Some(existing) = existing_receipt_by_id(&transaction, &assigned_id).await? {
            let receipt = replay_receipt_or_conflict(existing, &digest)?;
            transaction.commit().await?;
            return Ok(receipt);
        }

        let inserted = transaction
            .query_opt(
                "INSERT INTO administration_snapshot_receipts (receipt_id, request_id, outcome, \
                     reason, artifact_path, manifest_digest, encryption_key_id, size_bytes, \
                     verified, occurred_at, recorded_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) \
                 ON CONFLICT DO NOTHING RETURNING *",
                &[
                    &assigned_id,
                    &receipt.request_id,
                    &receipt.outcome.as_i16(),
                    &receipt.reason,
                    &receipt.artifact_path,
                    &receipt.manifest_digest,
                    &receipt.encryption_key_id,
                    &receipt.size_bytes,
                    &receipt.verified,
                    &receipt.occurred_at,
                    &now,
                ],
            )
            .await?;
        let receipt = match inserted {
            Some(row) => snapshot_receipt_from_row(&row)?,
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

    /// The receipt for a given request, if one has been recorded yet.
    pub async fn snapshot_receipt_for_request(
        &mut self,
        request_id: &str,
    ) -> Result<Option<SnapshotReceipt>, AdministrationStoreError> {
        let row = self
            .client
            .query_opt(
                "SELECT * FROM administration_snapshot_receipts WHERE request_id = $1",
                &[&request_id],
            )
            .await?;
        row.as_ref().map(snapshot_receipt_from_row).transpose()
    }

    /// The request itself, so a caller (the Bridge route) can check
    /// `requested_by` before disclosing its receipt to a different tenant
    /// token than the one that made it.
    pub async fn snapshot_request(
        &mut self,
        request_id: &str,
    ) -> Result<Option<SnapshotRequest>, AdministrationStoreError> {
        let row = self
            .client
            .query_opt(
                "SELECT * FROM administration_snapshot_requests WHERE request_id = $1",
                &[&request_id],
            )
            .await?;
        row.as_ref().map(snapshot_request_from_row).transpose()
    }
}

async fn existing_policy_by_id(
    transaction: &Transaction<'_>,
    policy_id: &str,
) -> Result<Option<AdministrationPolicy>, AdministrationStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM administration_policies WHERE policy_id = $1",
            &[&policy_id],
        )
        .await?
        .as_ref()
        .map(policy_from_row)
        .transpose()
}

fn replay_policy_or_conflict(
    policy: AdministrationPolicy,
    digest: &[u8],
) -> Result<PolicyWriteOutcome, AdministrationStoreError> {
    // The stored row carries no digest column of its own (unlike Work
    // commands): a policy's full content is already its identity input, so
    // recomputing it from the row and comparing catches any drift the same
    // way a stored digest column would, without a second column to keep in
    // sync.
    let recomputed = policy_digest(&PolicyAdoptionRequest {
        operation: policy.operation,
        scope: match &policy.scope {
            AdministrationScope::Platform => AdministrationScope::Platform,
            AdministrationScope::Tenant(id) => AdministrationScope::Tenant(id.clone()),
        },
        data_classification: policy.data_classification.clone(),
        retention_basis: policy.retention_basis.clone(),
        adopted_by: policy.adopted_by.clone(),
        idempotency_key: policy.idempotency_key.clone(),
        effective_at: policy.effective_at,
        expires_at: policy.expires_at,
    })?;
    if recomputed == digest {
        Ok(PolicyWriteOutcome {
            policy,
            idempotent_replay: true,
        })
    } else {
        Err(AdministrationStoreError::PolicyIdempotencyConflict)
    }
}

async fn existing_request_by_id(
    transaction: &Transaction<'_>,
    request_id: &str,
) -> Result<Option<SnapshotRequest>, AdministrationStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM administration_snapshot_requests WHERE request_id = $1",
            &[&request_id],
        )
        .await?
        .as_ref()
        .map(snapshot_request_from_row)
        .transpose()
}

fn replay_request_or_conflict(
    request: SnapshotRequest,
    digest: &[u8],
) -> Result<SnapshotRequestOutcome, AdministrationStoreError> {
    let recomputed = snapshot_request_digest(&NewSnapshotRequest {
        policy_id: request.policy_id.clone(),
        requested_by: request.requested_by.clone(),
        scope: match &request.scope {
            AdministrationScope::Platform => AdministrationScope::Platform,
            AdministrationScope::Tenant(id) => AdministrationScope::Tenant(id.clone()),
        },
        idempotency_key: request.idempotency_key.clone(),
    })?;
    if recomputed == digest {
        Ok(SnapshotRequestOutcome {
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
        .ok_or_else(|| AdministrationStoreError::UnknownRequest {
            request_id: request_id.to_string(),
        })
}

async fn existing_receipt_by_id(
    transaction: &Transaction<'_>,
    receipt_id: &str,
) -> Result<Option<SnapshotReceipt>, AdministrationStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM administration_snapshot_receipts WHERE receipt_id = $1",
            &[&receipt_id],
        )
        .await?
        .as_ref()
        .map(snapshot_receipt_from_row)
        .transpose()
}

fn replay_receipt_or_conflict(
    receipt: SnapshotReceipt,
    digest: &[u8],
) -> Result<SnapshotReceipt, AdministrationStoreError> {
    let recomputed = snapshot_receipt_digest(&NewSnapshotReceipt {
        request_id: receipt.request_id.clone(),
        outcome: receipt.outcome,
        reason: receipt.reason.clone(),
        artifact_path: receipt.artifact_path.clone(),
        manifest_digest: receipt.manifest_digest.clone(),
        encryption_key_id: receipt.encryption_key_id.clone(),
        size_bytes: receipt.size_bytes,
        verified: receipt.verified,
        occurred_at: receipt.occurred_at,
    })?;
    if recomputed == digest {
        Ok(receipt)
    } else {
        Err(AdministrationStoreError::ReceiptConflict)
    }
}

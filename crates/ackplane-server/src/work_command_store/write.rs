//! Transactional Work command and receipt persistence.

use std::time::SystemTime;

use tokio_postgres::Transaction;

use super::{
    model::{
        command_from_row, receipt_digest, receipt_from_row, request_digest, validate_receipt,
        validate_request,
    },
    NewWorkCommand, NewWorkCommandReceipt, WorkCommand, WorkCommandReceipt,
    WorkCommandReceiptWriteOutcome, WorkCommandStore, WorkCommandStoreError,
    WorkCommandWriteOutcome,
};

impl WorkCommandStore {
    /// Persists an immutable command request or returns its exact prior retry.
    pub async fn record_request(
        &mut self,
        request: &NewWorkCommand,
        now: SystemTime,
    ) -> Result<WorkCommandWriteOutcome, WorkCommandStoreError> {
        validate_request(request, now)?;
        let digest = request_digest(request)?;
        let transaction = self.client.transaction().await?;

        if let Some(command) = existing_by_command_id(&transaction, request).await? {
            let outcome = replay_or_conflict(command, &digest)?;
            transaction.commit().await?;
            return Ok(outcome);
        }
        if let Some(command) = existing_by_idempotency_key(&transaction, request).await? {
            let outcome = replay_or_conflict(command, &digest)?;
            transaction.commit().await?;
            return Ok(outcome);
        }

        let inserted = transaction
            .query_opt(
                "INSERT INTO work_commands (tenant_id, repository_id, command_id, command_kind, \
                     schema_version, task_id, issuing_principal_id, delegation_id, policy_refs, \
                     rationale, expected_task_version, confirmation_id, expires_at, idempotency_key, \
                     request_digest, payload_digest, recorded_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17) \
                 ON CONFLICT DO NOTHING RETURNING *",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.command_id,
                    &request.kind.as_i16(),
                    &request.schema_version,
                    &request.task_id,
                    &request.issuing_principal_id,
                    &request.delegation_id,
                    &request.policy_refs,
                    &request.rationale,
                    &request.expected_task_version,
                    &request.confirmation_id,
                    &request.expires_at,
                    &request.idempotency_key,
                    &digest,
                    &request.payload_digest,
                    &now,
                ],
            )
            .await?;
        let (command, idempotent_replay) = match inserted {
            Some(row) => (command_from_row(&row)?, false),
            None => {
                let command = existing_by_command_id(&transaction, request)
                    .await?
                    .or(existing_by_idempotency_key(&transaction, request).await?)
                    .ok_or(WorkCommandStoreError::IdempotencyConflict)?;
                let outcome = replay_or_conflict(command, &digest)?;
                (outcome.command, true)
            }
        };
        transaction.commit().await?;
        Ok(WorkCommandWriteOutcome {
            command,
            idempotent_replay,
        })
    }

    /// Appends one immutable command receipt or returns its exact prior retry.
    pub async fn record_receipt(
        &mut self,
        receipt: &NewWorkCommandReceipt,
        now: SystemTime,
    ) -> Result<WorkCommandReceiptWriteOutcome, WorkCommandStoreError> {
        validate_receipt(receipt, now)?;
        let digest = receipt_digest(receipt)?;
        let transaction = self.client.transaction().await?;
        ensure_command_exists(&transaction, receipt).await?;

        if let Some(existing) = existing_by_receipt_id(&transaction, receipt).await? {
            let outcome = replay_receipt_or_conflict(existing, &digest)?;
            transaction.commit().await?;
            return Ok(outcome);
        }
        if let Some(existing) = existing_by_receipt_digest(&transaction, receipt, &digest).await? {
            let outcome = replay_receipt_or_conflict(existing, &digest)?;
            transaction.commit().await?;
            return Ok(outcome);
        }

        let inserted = transaction
            .query_opt(
                "INSERT INTO work_command_receipts (tenant_id, repository_id, command_id, receipt_id, \
                     outcome, reason, evidence_refs, receipt_digest, occurred_at, recorded_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) \
                 ON CONFLICT DO NOTHING RETURNING *",
                &[
                    &receipt.tenant_id,
                    &receipt.repository_id,
                    &receipt.command_id,
                    &receipt.receipt_id,
                    &receipt.outcome.as_i16(),
                    &receipt.reason,
                    &receipt.evidence_refs,
                    &digest,
                    &receipt.occurred_at,
                    &now,
                ],
            )
            .await?;
        let (receipt, idempotent_replay) = match inserted {
            Some(row) => (receipt_from_row(&row)?, false),
            None => {
                let receipt = existing_by_receipt_id(&transaction, receipt)
                    .await?
                    .or(existing_by_receipt_digest(&transaction, receipt, &digest).await?)
                    .ok_or(WorkCommandStoreError::ReceiptConflict)?;
                let outcome = replay_receipt_or_conflict(receipt, &digest)?;
                (outcome.receipt, true)
            }
        };
        transaction.commit().await?;
        Ok(WorkCommandReceiptWriteOutcome {
            receipt,
            idempotent_replay,
        })
    }
}

async fn existing_by_command_id(
    transaction: &Transaction<'_>,
    request: &NewWorkCommand,
) -> Result<Option<WorkCommand>, WorkCommandStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM work_commands WHERE tenant_id = $1 AND repository_id = $2 \
             AND command_id = $3 FOR KEY SHARE",
            &[
                &request.tenant_id,
                &request.repository_id,
                &request.command_id,
            ],
        )
        .await?
        .map(|row| command_from_row(&row))
        .transpose()
}

async fn existing_by_idempotency_key(
    transaction: &Transaction<'_>,
    request: &NewWorkCommand,
) -> Result<Option<WorkCommand>, WorkCommandStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM work_commands WHERE tenant_id = $1 AND repository_id = $2 \
             AND issuing_principal_id = $3 AND idempotency_key = $4 FOR KEY SHARE",
            &[
                &request.tenant_id,
                &request.repository_id,
                &request.issuing_principal_id,
                &request.idempotency_key,
            ],
        )
        .await?
        .map(|row| command_from_row(&row))
        .transpose()
}

fn replay_or_conflict(
    command: WorkCommand,
    request_digest: &[u8],
) -> Result<WorkCommandWriteOutcome, WorkCommandStoreError> {
    if command.request_digest != request_digest {
        return Err(WorkCommandStoreError::IdempotencyConflict);
    }
    Ok(WorkCommandWriteOutcome {
        command,
        idempotent_replay: true,
    })
}

async fn ensure_command_exists(
    transaction: &Transaction<'_>,
    receipt: &NewWorkCommandReceipt,
) -> Result<(), WorkCommandStoreError> {
    let exists = transaction
        .query_opt(
            "SELECT 1 FROM work_commands WHERE tenant_id = $1 AND repository_id = $2 \
             AND command_id = $3 FOR KEY SHARE",
            &[
                &receipt.tenant_id,
                &receipt.repository_id,
                &receipt.command_id,
            ],
        )
        .await?
        .is_some();
    if exists {
        Ok(())
    } else {
        Err(WorkCommandStoreError::UnknownCommand {
            tenant_id: receipt.tenant_id.clone(),
            repository_id: receipt.repository_id.clone(),
            command_id: receipt.command_id.clone(),
        })
    }
}

async fn existing_by_receipt_id(
    transaction: &Transaction<'_>,
    receipt: &NewWorkCommandReceipt,
) -> Result<Option<WorkCommandReceipt>, WorkCommandStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM work_command_receipts WHERE tenant_id = $1 AND repository_id = $2 \
             AND receipt_id = $3 FOR KEY SHARE",
            &[
                &receipt.tenant_id,
                &receipt.repository_id,
                &receipt.receipt_id,
            ],
        )
        .await?
        .map(|row| receipt_from_row(&row))
        .transpose()
}

async fn existing_by_receipt_digest(
    transaction: &Transaction<'_>,
    receipt: &NewWorkCommandReceipt,
    digest: &[u8],
) -> Result<Option<WorkCommandReceipt>, WorkCommandStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM work_command_receipts WHERE tenant_id = $1 AND repository_id = $2 \
             AND command_id = $3 AND receipt_digest = $4 FOR KEY SHARE",
            &[
                &receipt.tenant_id,
                &receipt.repository_id,
                &receipt.command_id,
                &digest,
            ],
        )
        .await?
        .map(|row| receipt_from_row(&row))
        .transpose()
}

fn replay_receipt_or_conflict(
    receipt: WorkCommandReceipt,
    digest: &[u8],
) -> Result<WorkCommandReceiptWriteOutcome, WorkCommandStoreError> {
    if receipt.receipt_digest != digest {
        return Err(WorkCommandStoreError::ReceiptConflict);
    }
    Ok(WorkCommandReceiptWriteOutcome {
        receipt,
        idempotent_replay: true,
    })
}

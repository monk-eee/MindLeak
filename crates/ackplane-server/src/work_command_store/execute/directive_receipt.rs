//! Applies one supervisor's own typed directive receipt to the Work command
//! and task it addressed (ADR-0125 decision 7): only an `applied`, `refused`,
//! `failed`, or `expired` receipt for a directive this store itself issued
//! (see `supervisor_directives`) may append a Work event, and only `applied`
//! ever changes `work_tasks`' projected state. `accepted` -- a supervisor
//! merely acknowledging delivery, not yet having acted -- changes nothing
//! here; decision 7 names exactly the four outcomes that may.

use std::time::SystemTime;

use ackplane_protocol::v1;
use tokio_postgres::Transaction;

use super::{append_task_event, lock_task};
use crate::work_command_store::model::{
    command_from_row, receipt_from_row, NewWorkCommandReceipt, WorkCommand, WorkCommandKind,
    WorkCommandOutcome, WorkCommandReceiptWriteOutcome, WorkCommandStoreError,
};
use crate::work_command_store::write::append_receipt_in_transaction;
use crate::work_command_store::WorkCommandStore;

impl WorkCommandStore {
    /// Looks up the Work command that issued `receipt`'s directive (if any)
    /// and, for a terminal status, appends its receipt and Work event. `Ok(None)`
    /// means this directive was never issued through this store -- a Notify,
    /// Prompt, or Terminate directive, for instance -- and the caller's own
    /// `DirectiveStore::record_receipt` remains the sole durable record.
    pub async fn apply_directive_receipt(
        &mut self,
        receipt: &v1::DirectiveReceipt,
        now: SystemTime,
    ) -> Result<Option<WorkCommandReceiptWriteOutcome>, WorkCommandStoreError> {
        let transaction = self.client.transaction().await?;
        let outcome = apply_in_transaction(&transaction, receipt, now).await?;
        transaction.commit().await?;
        Ok(outcome)
    }
}

async fn apply_in_transaction(
    transaction: &Transaction<'_>,
    receipt: &v1::DirectiveReceipt,
    now: SystemTime,
) -> Result<Option<WorkCommandReceiptWriteOutcome>, WorkCommandStoreError> {
    let Some(outcome) = terminal_outcome(receipt.status) else {
        return Ok(None);
    };
    let Some(command) = command_by_directive_id(
        transaction,
        &receipt.tenant_id,
        &receipt.repository_id,
        &receipt.directive_id,
    )
    .await?
    else {
        return Ok(None);
    };

    let receipt_id = format!("{}:directive", command.command_id);
    if let Some(existing) = existing_receipt(
        transaction,
        &command.tenant_id,
        &command.repository_id,
        &receipt_id,
    )
    .await?
    {
        return Ok(Some(WorkCommandReceiptWriteOutcome {
            receipt: existing,
            idempotent_replay: true,
        }));
    }

    if outcome == WorkCommandOutcome::Applied {
        apply_task_effect(transaction, &command, now).await?;
    } else {
        // Refused/Failed/Expired still leave a visible, durable audit trail
        // (acceptance criterion 4): the task's state does not move, but the
        // attempt is recorded, exactly like `RouteWork`'s unchanged-state event.
        if let Some(task_id) = command.task_id.as_deref() {
            if let Some(current) = lock_task(
                transaction,
                &command.tenant_id,
                &command.repository_id,
                task_id,
            )
            .await?
            {
                append_task_event(
                    transaction,
                    &command,
                    task_id,
                    event_kind(command.kind),
                    current.state,
                    current.state,
                    now,
                )
                .await?;
            }
        }
    }

    let new_receipt = NewWorkCommandReceipt {
        tenant_id: command.tenant_id.clone(),
        repository_id: command.repository_id.clone(),
        command_id: command.command_id.clone(),
        receipt_id,
        outcome,
        reason: directive_receipt_reason(receipt),
        evidence_refs: receipt.evidence_refs.clone(),
        occurred_at: now,
    };
    let written = append_receipt_in_transaction(transaction, &new_receipt, now).await?;
    Ok(Some(written))
}

fn terminal_outcome(status: i32) -> Option<WorkCommandOutcome> {
    match v1::DirectiveReceiptStatus::try_from(status).ok()? {
        v1::DirectiveReceiptStatus::Applied => Some(WorkCommandOutcome::Applied),
        v1::DirectiveReceiptStatus::Refused => Some(WorkCommandOutcome::Refused),
        v1::DirectiveReceiptStatus::Failed => Some(WorkCommandOutcome::Failed),
        v1::DirectiveReceiptStatus::Expired => Some(WorkCommandOutcome::Expired),
        v1::DirectiveReceiptStatus::Accepted | v1::DirectiveReceiptStatus::Unspecified => None,
    }
}

fn directive_receipt_reason(receipt: &v1::DirectiveReceipt) -> String {
    if receipt.diagnostic.is_empty() {
        format!(
            "supervisor directive receipt: status={}, reason={}",
            receipt.status, receipt.reason
        )
    } else {
        receipt.diagnostic.clone()
    }
}

fn event_kind(kind: WorkCommandKind) -> i16 {
    // Mirrors WorkCommandKind::as_i16 (6-10) so a Work event's kind names
    // exactly which supervisor-directed command produced it.
    kind.as_i16()
}

/// `Applied` is the only status that moves `work_tasks`' own projection.
/// Assign and Steer address a specific supervisor session directly, so this
/// applies unconditionally rather than re-deriving a target the command's own
/// payload already fixed at confirm time.
async fn apply_task_effect(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    now: SystemTime,
) -> Result<(), WorkCommandStoreError> {
    let Some(task_id) = command.task_id.as_deref() else {
        return Ok(());
    };
    let Some(current) = lock_task(
        transaction,
        &command.tenant_id,
        &command.repository_id,
        task_id,
    )
    .await?
    else {
        return Ok(());
    };

    const COMPLETED: i16 = 7;
    const ABANDONED: i16 = 8;
    if matches!(current.state, COMPLETED | ABANDONED) {
        // The supervisor may finish a directive after another path terminally
        // resolves its Work task. Preserve that terminal projection while the
        // caller still records the supervisor's immutable Applied receipt.
        return Ok(());
    }

    const OPEN: i16 = 1;
    const CLAIMED: i16 = 2;
    const PAUSED: i16 = 4;

    let next_state = match command.kind {
        WorkCommandKind::Pause => PAUSED,
        WorkCommandKind::Resume => CLAIMED,
        WorkCommandKind::Drain => OPEN,
        WorkCommandKind::Assign => CLAIMED,
        _ => current.state,
    };

    match command.kind {
        WorkCommandKind::Drain => {
            transaction
                .execute(
                    "UPDATE work_tasks SET owner_id = NULL, owner_session_id = NULL, \
                         lease_expires_at = NULL, state = $4, version = version + 1, updated_at = $5 \
                     WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
                    &[
                        &command.tenant_id,
                        &command.repository_id,
                        &task_id,
                        &next_state,
                        &now,
                    ],
                )
                .await?;
        }
        WorkCommandKind::Assign => {
            transaction
                .execute(
                    "UPDATE work_tasks SET state = $4, version = version + 1, updated_at = $5 \
                     WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
                    &[
                        &command.tenant_id,
                        &command.repository_id,
                        &task_id,
                        &next_state,
                        &now,
                    ],
                )
                .await?;
        }
        _ if next_state != current.state => {
            transaction
                .execute(
                    "UPDATE work_tasks SET state = $4, version = version + 1, updated_at = $5 \
                     WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
                    &[
                        &command.tenant_id,
                        &command.repository_id,
                        &task_id,
                        &next_state,
                        &now,
                    ],
                )
                .await?;
        }
        _ => {
            // Steer: no state transition, but still bump version and record
            // history, exactly like `RouteWork`.
            transaction
                .execute(
                    "UPDATE work_tasks SET version = version + 1, updated_at = $4 \
                     WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
                    &[&command.tenant_id, &command.repository_id, &task_id, &now],
                )
                .await?;
        }
    }

    append_task_event(
        transaction,
        command,
        task_id,
        event_kind(command.kind),
        current.state,
        next_state,
        now,
    )
    .await
}

async fn command_by_directive_id(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    repository_id: &str,
    directive_id: &str,
) -> Result<Option<WorkCommand>, WorkCommandStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM work_commands \
             WHERE tenant_id = $1 AND repository_id = $2 AND directive_id = $3 FOR UPDATE",
            &[&tenant_id, &repository_id, &directive_id],
        )
        .await?
        .map(|row| command_from_row(&row))
        .transpose()
}

async fn existing_receipt(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    repository_id: &str,
    receipt_id: &str,
) -> Result<Option<crate::work_command_store::model::WorkCommandReceipt>, WorkCommandStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM work_command_receipts \
             WHERE tenant_id = $1 AND repository_id = $2 AND receipt_id = $3 FOR KEY SHARE",
            &[&tenant_id, &repository_id, &receipt_id],
        )
        .await?
        .map(|row| receipt_from_row(&row))
        .transpose()
}

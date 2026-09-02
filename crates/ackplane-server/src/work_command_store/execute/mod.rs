//! Executes a confirmed Work command's server-owned effect (ADR-0125
//! decisions 5-6): the Work/Claim mutation and the resulting receipt commit
//! or roll back together in one transaction, on `WorkCommandStore`'s own
//! connection -- never through a second store's separate connection, since
//! that would make true cross-table atomicity impossible.
//!
//! `CreateWork`/`RouteWork`/`AnswerWait`/`SubmitReview` mutate `work_tasks`/
//! `work_task_history`/`work_task_waits` directly; `ReleaseLease` also
//! mutates `delegated_claims`/`delegated_claim_history` in the SAME
//! transaction, preserving `ClaimStore` as the sole lease authority (decision
//! 6) while still committing atomically with the command receipt.

use std::time::SystemTime;

use sha2::{Digest, Sha256};
use tokio_postgres::Transaction;

use super::{
    model::{
        receipt_from_row, NewWorkCommandReceipt, WorkCommand, WorkCommandOutcome,
        WorkCommandReceipt, WorkCommandReceiptWriteOutcome, WorkCommandStoreError,
    },
    payload::{payload_digest, payload_matches_kind, WorkCommandPayload},
    write::append_receipt_in_transaction,
    WorkCommandStore,
};

mod create_work;
mod directive_receipt;
mod release_lease;
mod supervisor_directives;
mod task_mutations;

/// What actually happened applying a confirmed command's payload. Recorded
/// as the resulting receipt's outcome and reason; never a Rust `Err` -- a
/// stale version or changed claim is a legitimate, expected, typed result,
/// not a defect.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ExecutionOutcome {
    Applied,
    Expired,
    PayloadMismatch,
    TaskAlreadyExists,
    TaskNotFound,
    TaskVersionConflict {
        current_version: i64,
    },
    WaitNotFound,
    WaitAlreadyAnswered,
    ClaimMissing,
    ClaimOwnerChanged {
        current_owner_id: String,
    },
    ClaimLeaseChanged {
        current_lease_expires_at: SystemTime,
    },
    /// The directive was durably enqueued to the addressed supervisor
    /// (ADR-0125 decision 7); the receipt records `PendingDelivery`, never
    /// `Applied` -- only the supervisor's own later receipt may apply an
    /// effect (see `directive_receipt::apply`).
    DirectiveIssued {
        directive_id: String,
    },
    /// The addressed supervisor session does not advertise the directive
    /// kind's required capability.
    SupervisorCapabilityMissing,
    /// No enrolled supervisor session matches the addressed target.
    SupervisorSessionUnknown,
    /// The addressed session belongs to a different node than named.
    SupervisorTargetMismatch,
}

impl WorkCommandStore {
    /// Confirms and executes one already-recorded command's effect, or
    /// replays its exact prior confirmation. Idempotent on the command id:
    /// a lost response is safe to retry without re-applying the effect.
    pub(super) async fn execute_confirmed(
        &self,
        command: &WorkCommand,
        payload: &WorkCommandPayload,
        now: SystemTime,
    ) -> Result<WorkCommandReceiptWriteOutcome, WorkCommandStoreError> {
        let receipt_id = format!("{}:confirm", command.command_id);
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        if let Some(existing) = existing_confirm_receipt(
            &transaction,
            &command.tenant_id,
            &command.repository_id,
            &receipt_id,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(WorkCommandReceiptWriteOutcome {
                receipt: existing,
                idempotent_replay: true,
            });
        }

        let outcome = if now >= command.expires_at {
            ExecutionOutcome::Expired
        } else if payload_matches_kind(payload, command.kind)
            && payload_digest(payload)? == command.payload_digest
        {
            apply_effect(&transaction, command, payload, now).await?
        } else {
            ExecutionOutcome::PayloadMismatch
        };
        let (outcome_kind, reason) = describe(&outcome, now);
        let receipt = NewWorkCommandReceipt {
            tenant_id: command.tenant_id.clone(),
            repository_id: command.repository_id.clone(),
            command_id: command.command_id.clone(),
            receipt_id,
            outcome: outcome_kind,
            reason,
            evidence_refs: Vec::new(),
            occurred_at: now,
        };
        let written = append_receipt_in_transaction(&transaction, &receipt, now).await?;
        transaction.commit().await?;
        Ok(written)
    }
}

async fn existing_confirm_receipt(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    repository_id: &str,
    receipt_id: &str,
) -> Result<Option<WorkCommandReceipt>, WorkCommandStoreError> {
    transaction
        .query_opt(
            "SELECT * FROM work_command_receipts WHERE tenant_id = $1 AND repository_id = $2 \
             AND receipt_id = $3 FOR KEY SHARE",
            &[&tenant_id, &repository_id, &receipt_id],
        )
        .await?
        .map(|row| receipt_from_row(&row))
        .transpose()
}

async fn apply_effect(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    payload: &WorkCommandPayload,
    now: SystemTime,
) -> Result<ExecutionOutcome, WorkCommandStoreError> {
    match payload {
        WorkCommandPayload::CreateWork(payload) => {
            create_work::create_work(transaction, command, payload, now).await
        }
        WorkCommandPayload::RouteWork(payload) => {
            task_mutations::route_work(transaction, command, payload, now).await
        }
        WorkCommandPayload::ReleaseLease(payload) => {
            release_lease::release_lease(transaction, command, payload, now).await
        }
        WorkCommandPayload::AnswerWait(payload) => {
            task_mutations::answer_wait(transaction, command, payload, now).await
        }
        WorkCommandPayload::SubmitReview(payload) => {
            task_mutations::submit_review(transaction, command, payload, now).await
        }
        WorkCommandPayload::Assign(payload) => {
            supervisor_directives::assign(transaction, command, payload, now).await
        }
        WorkCommandPayload::Steer(payload) => {
            supervisor_directives::steer(transaction, command, payload, now).await
        }
        WorkCommandPayload::Pause(payload) => {
            supervisor_directives::pause(transaction, command, payload, now).await
        }
        WorkCommandPayload::Resume(payload) => {
            supervisor_directives::resume(transaction, command, payload, now).await
        }
        WorkCommandPayload::Drain(payload) => {
            supervisor_directives::drain(transaction, command, payload, now).await
        }
    }
}

fn describe(outcome: &ExecutionOutcome, now: SystemTime) -> (WorkCommandOutcome, String) {
    match outcome {
        ExecutionOutcome::Applied => (
            WorkCommandOutcome::Applied,
            "The command's server-owned effect was applied.".to_owned(),
        ),
        ExecutionOutcome::Expired => (
            WorkCommandOutcome::Expired,
            "The command's confirmation window has expired; a new preview is required.".to_owned(),
        ),
        ExecutionOutcome::PayloadMismatch => (
            WorkCommandOutcome::Refused,
            "The confirmed payload does not match the digest recorded at submission; a changed \
             field requires a new preview."
                .to_owned(),
        ),
        ExecutionOutcome::TaskAlreadyExists => (
            WorkCommandOutcome::Conflicted,
            "A task with this task_id already exists.".to_owned(),
        ),
        ExecutionOutcome::TaskNotFound => (
            WorkCommandOutcome::Refused,
            "The named task does not exist in this tenant and repository.".to_owned(),
        ),
        ExecutionOutcome::TaskVersionConflict { current_version } => (
            WorkCommandOutcome::Conflicted,
            format!(
                "expected_task_version is stale; the task's current version is {current_version}."
            ),
        ),
        ExecutionOutcome::WaitNotFound => (
            WorkCommandOutcome::Refused,
            "The named wait does not exist on this task.".to_owned(),
        ),
        ExecutionOutcome::WaitAlreadyAnswered => (
            WorkCommandOutcome::Conflicted,
            "The named wait was already answered.".to_owned(),
        ),
        ExecutionOutcome::ClaimMissing => (
            WorkCommandOutcome::Refused,
            "No claim exists for this task.".to_owned(),
        ),
        ExecutionOutcome::ClaimOwnerChanged { current_owner_id } => (
            WorkCommandOutcome::Conflicted,
            format!("the claim's current owner is {current_owner_id}, not the expected owner."),
        ),
        ExecutionOutcome::ClaimLeaseChanged {
            current_lease_expires_at,
        } => {
            let detail = if *current_lease_expires_at < now {
                "expired"
            } else if *current_lease_expires_at == now {
                "is still live"
            } else {
                "was renewed"
            };
            (
                WorkCommandOutcome::Conflicted,
                format!("the claim's lease {detail}; it no longer matches the observed lease."),
            )
        }
        ExecutionOutcome::DirectiveIssued { directive_id } => (
            WorkCommandOutcome::PendingDelivery,
            format!(
                "directive {directive_id} was durably enqueued to the addressed supervisor; \
                 only its own applied/refused/failed/expired receipt may change the task."
            ),
        ),
        ExecutionOutcome::SupervisorCapabilityMissing => (
            WorkCommandOutcome::Refused,
            "the addressed supervisor session does not advertise this directive's required \
             capability."
                .to_owned(),
        ),
        ExecutionOutcome::SupervisorSessionUnknown => (
            WorkCommandOutcome::Refused,
            "no enrolled supervisor session matches the addressed target.".to_owned(),
        ),
        ExecutionOutcome::SupervisorTargetMismatch => (
            WorkCommandOutcome::Refused,
            "the addressed session belongs to a different node than named.".to_owned(),
        ),
    }
}

fn event_digest(command_id: &str, event_kind: i16, to_state: i16) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(command_id.as_bytes());
    hasher.update(event_kind.to_be_bytes());
    hasher.update(to_state.to_be_bytes());
    hasher.finalize().to_vec()
}

/// Appends one lifecycle event to the repository's Work stream and points the
/// task's projection at it.
///
/// A Work command's effect is an event like any other (ADR-0120 decision 3),
/// so it takes the next position from the same per-repository head that
/// creation does. Stamping `source_event_position` here rather than in each of
/// the eight command paths that update `work_tasks` means the projection cannot
/// drift from the event that produced it: there is one write site, and every
/// path that appends an event goes through it.
async fn append_task_event(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    task_id: &str,
    event_kind: i16,
    from_state: i16,
    to_state: i16,
    now: SystemTime,
) -> Result<(), WorkCommandStoreError> {
    let stream_position = crate::work_store::allocate_stream_position(
        transaction,
        &command.tenant_id,
        &command.repository_id,
    )
    .await?;
    transaction
        .execute(
            "INSERT INTO work_task_history (tenant_id, repository_id, event_id, task_id, \
                 event_kind, from_state, to_state, actor_id, source_digest, stream_position, \
                 recorded_at) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
            &[
                &command.tenant_id,
                &command.repository_id,
                &command.command_id,
                &task_id,
                &event_kind,
                &from_state,
                &to_state,
                &command.issuing_principal_id,
                &event_digest(&command.command_id, event_kind, to_state),
                &stream_position,
                &now,
            ],
        )
        .await?;
    transaction
        .execute(
            "UPDATE work_tasks SET source_event_position = $4 \
             WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
            &[
                &command.tenant_id,
                &command.repository_id,
                &task_id,
                &stream_position,
            ],
        )
        .await?;
    Ok(())
}

struct LockedTask {
    state: i16,
    version: i64,
}

async fn lock_task(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    repository_id: &str,
    task_id: &str,
) -> Result<Option<LockedTask>, WorkCommandStoreError> {
    Ok(transaction
        .query_opt(
            "SELECT state, version FROM work_tasks \
             WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 FOR UPDATE",
            &[&tenant_id, &repository_id, &task_id],
        )
        .await?
        .map(|row| LockedTask {
            state: row.get("state"),
            version: row.get("version"),
        }))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;

    #[test]
    fn a_changed_lease_at_its_exact_expiry_is_not_reported_as_expired() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let (outcome, reason) = describe(
            &ExecutionOutcome::ClaimLeaseChanged {
                current_lease_expires_at: now,
            },
            now,
        );

        assert_eq!(outcome, WorkCommandOutcome::Conflicted);
        assert!(
            reason.contains("is still live"),
            "the local authority defines equality as live: {reason}"
        );
    }
}

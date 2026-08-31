//! Issues the ADR-0107 directive Assign/Steer/Pause/Resume/Drain request
//! (ADR-0125 decision 7), on `WorkCommandStore`'s own transaction so the
//! directive and its command receipt commit or roll back together -- the
//! same cross-table atomicity `release_lease` already relies on, extended to
//! a second store's tables via `directive_store::enqueue_in_transaction`
//! rather than a second connection.
//!
//! A supervisor-directed command's confirm step never mutates `work_tasks`
//! itself (decision 7: "not immediate Work transitions"). That happens only
//! when the addressed supervisor's own typed directive receipt later arrives
//! -- see `directive_receipt::apply`.

use std::time::{Duration, SystemTime};

use ackplane_protocol::{
    supervisor::{directive_payload_digest, directive_requirement},
    v1::{self, agent_directive},
};
use tokio_postgres::Transaction;

use super::ExecutionOutcome;
use crate::directive_store::{self, DirectiveStoreError, DirectiveWriteOutcome};
use crate::work_command_store::model::{WorkCommand, WorkCommandStoreError};
use crate::work_command_store::payload::{
    AssignPayload, DirectiveTarget, DrainPayload, PausePayload, ResumePayload, SteerPayload,
};

/// A directive's own project dimension has no equivalent on `WorkCommand`;
/// Work tasks are not modeled per-project. A fixed, deterministic,
/// repository-scoped placeholder satisfies the directive envelope's
/// required, non-empty `project_id` without inventing per-task project
/// tracking this domain does not have.
fn synthetic_project_id(repository_id: &str) -> String {
    format!("project:work-commands:{repository_id}")
}

fn directive_id_for(command: &WorkCommand) -> String {
    format!("directive:work-command:{}", command.command_id)
}

fn rfc3339(value: SystemTime) -> Result<String, WorkCommandStoreError> {
    crate::wire_format::rfc3339(value).map_err(|_| WorkCommandStoreError::InvalidTimestamp)
}

/// Builds, validates, and enqueues one directive for `command`, then records
/// its id on the command row. Returns the typed outcome the receipt (never
/// `Applied` -- only the supervisor's own later receipt may apply an effect)
/// should carry.
#[allow(clippy::too_many_arguments)]
async fn issue(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    target: &DirectiveTarget,
    kind: v1::DirectiveKind,
    payload: agent_directive::Payload,
    now: SystemTime,
    expires_at: SystemTime,
) -> Result<ExecutionOutcome, WorkCommandStoreError> {
    let Some(task_id) = command.task_id.clone() else {
        return Ok(ExecutionOutcome::TaskNotFound);
    };
    let mut directive = v1::AgentDirective {
        directive_id: directive_id_for(command),
        tenant_id: command.tenant_id.clone(),
        project_id: synthetic_project_id(&command.repository_id),
        repository_id: command.repository_id.clone(),
        target_node_id: target.target_node_id.clone(),
        target_agent_session_id: target.target_session_id.clone(),
        kind: kind as i32,
        schema_version: command.schema_version.clone(),
        issuing_principal_id: command.issuing_principal_id.clone(),
        rationale: command.rationale.clone(),
        task_id,
        goal_id: String::new(),
        context_packet_id: String::new(),
        created_at: String::new(),
        expires_at: rfc3339(expires_at)?,
        sequence: 0,
        idempotency_key: command.idempotency_key.clone(),
        payload_digest: Vec::new(),
        required_capability: String::new(),
        policy_refs: command.policy_refs.clone(),
        knowledge_refs: Vec::new(),
        evidence_refs: Vec::new(),
        payload: Some(payload),
    };
    let requirement =
        directive_requirement(&directive).ok_or(WorkCommandStoreError::InvalidDirectivePayload)?;
    directive.required_capability = requirement.required_capability.to_string();
    directive.payload_digest = directive_payload_digest(&directive)
        .ok_or(WorkCommandStoreError::InvalidDirectivePayload)?;

    match directive_store::enqueue_in_transaction(transaction, directive, now).await {
        Ok(DirectiveWriteOutcome { record, .. }) => {
            persist_directive_id(transaction, command, &record.directive.directive_id).await?;
            Ok(ExecutionOutcome::DirectiveIssued {
                directive_id: record.directive.directive_id,
            })
        }
        Err(DirectiveStoreError::CapabilityMissing) => {
            Ok(ExecutionOutcome::SupervisorCapabilityMissing)
        }
        Err(DirectiveStoreError::UnknownSupervisorSession) => {
            Ok(ExecutionOutcome::SupervisorSessionUnknown)
        }
        Err(DirectiveStoreError::TargetNodeMismatch) => {
            Ok(ExecutionOutcome::SupervisorTargetMismatch)
        }
        Err(DirectiveStoreError::Database(error)) => Err(WorkCommandStoreError::Database(error)),
        Err(error) => Err(WorkCommandStoreError::Directive(error)),
    }
}

async fn persist_directive_id(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    directive_id: &str,
) -> Result<(), WorkCommandStoreError> {
    transaction
        .execute(
            "UPDATE work_commands SET directive_id = $4 \
             WHERE tenant_id = $1 AND repository_id = $2 AND command_id = $3",
            &[
                &command.tenant_id,
                &command.repository_id,
                &command.command_id,
                &directive_id,
            ],
        )
        .await?;
    Ok(())
}

/// Directives carry their own expiry independent of the command's; a
/// directive not delivered and acted on within this window is the
/// supervisor's own concern to report `expired`, not the command's.
const DIRECTIVE_LIFETIME: Duration = Duration::from_secs(3_600);

pub(super) async fn assign(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    payload: &AssignPayload,
    now: SystemTime,
) -> Result<ExecutionOutcome, WorkCommandStoreError> {
    issue(
        transaction,
        command,
        &payload.target,
        v1::DirectiveKind::Assign,
        agent_directive::Payload::Assign(v1::AssignDirective {}),
        now,
        now + DIRECTIVE_LIFETIME,
    )
    .await
}

pub(super) async fn steer(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    payload: &SteerPayload,
    now: SystemTime,
) -> Result<ExecutionOutcome, WorkCommandStoreError> {
    issue(
        transaction,
        command,
        &payload.target,
        v1::DirectiveKind::Steer,
        agent_directive::Payload::Steer(v1::SteerDirective {
            instruction: payload.instruction.clone(),
            checkpoint_required: payload.checkpoint_required,
        }),
        now,
        now + DIRECTIVE_LIFETIME,
    )
    .await
}

pub(super) async fn pause(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    payload: &PausePayload,
    now: SystemTime,
) -> Result<ExecutionOutcome, WorkCommandStoreError> {
    issue(
        transaction,
        command,
        &payload.target,
        v1::DirectiveKind::Pause,
        agent_directive::Payload::Pause(v1::PauseDirective {
            checkpoint_required: payload.checkpoint_required,
        }),
        now,
        now + DIRECTIVE_LIFETIME,
    )
    .await
}

pub(super) async fn resume(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    payload: &ResumePayload,
    now: SystemTime,
) -> Result<ExecutionOutcome, WorkCommandStoreError> {
    issue(
        transaction,
        command,
        &payload.target,
        v1::DirectiveKind::Resume,
        agent_directive::Payload::Resume(v1::ResumeDirective {}),
        now,
        now + DIRECTIVE_LIFETIME,
    )
    .await
}

pub(super) async fn drain(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    payload: &DrainPayload,
    now: SystemTime,
) -> Result<ExecutionOutcome, WorkCommandStoreError> {
    issue(
        transaction,
        command,
        &payload.target,
        v1::DirectiveKind::Drain,
        agent_directive::Payload::Drain(v1::DrainDirective {
            deadline: rfc3339(payload.deadline)?,
        }),
        now,
        payload.deadline.max(now + DIRECTIVE_LIFETIME),
    )
    .await
}

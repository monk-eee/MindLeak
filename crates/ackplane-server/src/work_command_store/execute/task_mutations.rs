use std::time::SystemTime;

use tokio_postgres::Transaction;

use super::ExecutionOutcome;
use crate::work_command_store::model::{WorkCommand, WorkCommandStoreError};
use crate::work_command_store::payload::{
    AnswerWaitPayload, RouteWorkPayload, SubmitReviewPayload,
};

pub(super) async fn route_work(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    payload: &RouteWorkPayload,
    now: SystemTime,
) -> Result<ExecutionOutcome, WorkCommandStoreError> {
    let (Some(task_id), Some(expected_version)) =
        (command.task_id.as_deref(), command.expected_task_version)
    else {
        return Ok(ExecutionOutcome::TaskNotFound);
    };
    let Some(current) = super::lock_task(
        transaction,
        &command.tenant_id,
        &command.repository_id,
        task_id,
    )
    .await?
    else {
        return Ok(ExecutionOutcome::TaskNotFound);
    };
    if current.version != expected_version {
        return Ok(ExecutionOutcome::TaskVersionConflict {
            current_version: current.version,
        });
    }
    transaction
        .execute(
            "UPDATE work_tasks SET route_reference = $4, version = version + 1, updated_at = $5 \
             WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
            &[
                &command.tenant_id,
                &command.repository_id,
                &task_id,
                &payload.route_reference,
                &now,
            ],
        )
        .await?;
    super::append_task_event(
        transaction,
        command,
        task_id,
        2,
        current.state,
        current.state,
        now,
    )
    .await?;
    Ok(ExecutionOutcome::Applied)
}

pub(super) async fn answer_wait(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    payload: &AnswerWaitPayload,
    now: SystemTime,
) -> Result<ExecutionOutcome, WorkCommandStoreError> {
    let (Some(task_id), Some(expected_version)) =
        (command.task_id.as_deref(), command.expected_task_version)
    else {
        return Ok(ExecutionOutcome::TaskNotFound);
    };
    let Some(current) = super::lock_task(
        transaction,
        &command.tenant_id,
        &command.repository_id,
        task_id,
    )
    .await?
    else {
        return Ok(ExecutionOutcome::TaskNotFound);
    };
    if current.version != expected_version {
        return Ok(ExecutionOutcome::TaskVersionConflict {
            current_version: current.version,
        });
    }
    let wait = transaction
        .query_opt(
            "SELECT answer FROM work_task_waits \
             WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 AND wait_id = $4 \
             FOR UPDATE",
            &[
                &command.tenant_id,
                &command.repository_id,
                &task_id,
                &payload.wait_id,
            ],
        )
        .await?;
    let Some(wait) = wait else {
        return Ok(ExecutionOutcome::WaitNotFound);
    };
    let already_answered: Option<String> = wait.get("answer");
    if already_answered.is_some() {
        return Ok(ExecutionOutcome::WaitAlreadyAnswered);
    }
    transaction
        .execute(
            "UPDATE work_task_waits SET answered_by = $5, answer = $6, answered_at = $7 \
             WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 AND wait_id = $4",
            &[
                &command.tenant_id,
                &command.repository_id,
                &task_id,
                &payload.wait_id,
                &command.issuing_principal_id,
                &payload.answer,
                &now,
            ],
        )
        .await?;
    transaction
        .execute(
            "UPDATE work_tasks SET version = version + 1, updated_at = $4 \
             WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
            &[&command.tenant_id, &command.repository_id, &task_id, &now],
        )
        .await?;
    super::append_task_event(
        transaction,
        command,
        task_id,
        5,
        current.state,
        current.state,
        now,
    )
    .await?;
    Ok(ExecutionOutcome::Applied)
}

pub(super) async fn submit_review(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    _payload: &SubmitReviewPayload,
    now: SystemTime,
) -> Result<ExecutionOutcome, WorkCommandStoreError> {
    let (Some(task_id), Some(expected_version)) =
        (command.task_id.as_deref(), command.expected_task_version)
    else {
        return Ok(ExecutionOutcome::TaskNotFound);
    };
    let Some(current) = super::lock_task(
        transaction,
        &command.tenant_id,
        &command.repository_id,
        task_id,
    )
    .await?
    else {
        return Ok(ExecutionOutcome::TaskNotFound);
    };
    if current.version != expected_version {
        return Ok(ExecutionOutcome::TaskVersionConflict {
            current_version: current.version,
        });
    }
    const IN_REVIEW: i16 = 6;
    transaction
        .execute(
            "UPDATE work_tasks SET state = $4, version = version + 1, updated_at = $5 \
             WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
            &[
                &command.tenant_id,
                &command.repository_id,
                &task_id,
                &IN_REVIEW,
                &now,
            ],
        )
        .await?;
    super::append_task_event(
        transaction,
        command,
        task_id,
        4,
        current.state,
        IN_REVIEW,
        now,
    )
    .await?;
    Ok(ExecutionOutcome::Applied)
}

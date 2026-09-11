use std::time::SystemTime;

use tokio_postgres::Transaction;

use super::ExecutionOutcome;
use crate::work_command_store::model::{WorkCommand, WorkCommandStoreError};
use crate::work_command_store::payload::{payload_digest, CreateWorkPayload, WorkCommandPayload};

pub(super) async fn create_work(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    payload: &CreateWorkPayload,
    now: SystemTime,
) -> Result<ExecutionOutcome, WorkCommandStoreError> {
    let digest = payload_digest(&WorkCommandPayload::CreateWork(payload.clone()))?;
    let inserted = transaction
        .execute(
            "INSERT INTO work_tasks (tenant_id, repository_id, task_id, title, acceptance, \
                 goal_id, state, declared_paths, declared_symbols, source_digest, published_by, \
                 version, created_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,1,$10,$11,$7,$8,1,$9,$9) \
             ON CONFLICT (tenant_id, repository_id, task_id) DO NOTHING",
            &[
                &command.tenant_id,
                &command.repository_id,
                &payload.task_id,
                &payload.title,
                &payload.acceptance,
                &payload.goal_id,
                &digest,
                &command.issuing_principal_id,
                &now,
                &payload.declared_paths,
                &payload.declared_symbols,
            ],
        )
        .await?;
    if inserted == 0 {
        return Ok(ExecutionOutcome::TaskAlreadyExists);
    }
    super::append_task_event(transaction, command, &payload.task_id, 1, 1, 1, now).await?;
    Ok(ExecutionOutcome::Applied)
}

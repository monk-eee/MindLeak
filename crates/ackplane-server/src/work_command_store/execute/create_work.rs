use std::time::SystemTime;

use tokio_postgres::Transaction;

use super::ExecutionOutcome;
use crate::work_command_store::model::{WorkCommand, WorkCommandStoreError};
use crate::work_command_store::payload::CreateWorkPayload;

fn create_work_digest(payload: &CreateWorkPayload) -> Vec<u8> {
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    hasher.update(payload.task_id.as_bytes());
    hasher.update(payload.title.as_bytes());
    hasher.update(payload.acceptance.as_bytes());
    if let Some(goal_id) = &payload.goal_id {
        hasher.update(goal_id.as_bytes());
    }
    hasher.finalize().to_vec()
}

pub(super) async fn create_work(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    payload: &CreateWorkPayload,
    now: SystemTime,
) -> Result<ExecutionOutcome, WorkCommandStoreError> {
    let digest = create_work_digest(payload);
    let inserted = transaction
        .execute(
            "INSERT INTO work_tasks (tenant_id, repository_id, task_id, title, acceptance, \
                 goal_id, state, declared_paths, declared_symbols, source_digest, published_by, \
                 version, created_at, updated_at) \
             VALUES ($1,$2,$3,$4,$5,$6,1,ARRAY[]::text[],ARRAY[]::text[],$7,$8,1,$9,$9) \
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
            ],
        )
        .await?;
    if inserted == 0 {
        return Ok(ExecutionOutcome::TaskAlreadyExists);
    }
    super::append_task_event(transaction, command, &payload.task_id, 1, 1, 1, now).await?;
    Ok(ExecutionOutcome::Applied)
}

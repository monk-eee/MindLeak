use std::time::SystemTime;

use tokio_postgres::Transaction;

use super::ExecutionOutcome;
use crate::work_command_store::model::{WorkCommand, WorkCommandStoreError};
use crate::work_command_store::payload::ReleaseLeasePayload;

pub(super) async fn release_lease(
    transaction: &Transaction<'_>,
    command: &WorkCommand,
    payload: &ReleaseLeasePayload,
    now: SystemTime,
) -> Result<ExecutionOutcome, WorkCommandStoreError> {
    let Some(task_id) = command.task_id.as_deref() else {
        return Ok(ExecutionOutcome::ClaimMissing);
    };
    let claim = transaction
        .query_opt(
            "SELECT owner_id, lease_expires_at FROM delegated_claims \
             WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 FOR UPDATE",
            &[&command.tenant_id, &command.repository_id, &task_id],
        )
        .await?;
    let Some(claim) = claim else {
        return Ok(ExecutionOutcome::ClaimMissing);
    };
    let current_owner_id: String = claim.get("owner_id");
    let current_lease_expires_at: SystemTime = claim.get("lease_expires_at");
    if current_owner_id != payload.expected_owner_id {
        return Ok(ExecutionOutcome::ClaimOwnerChanged { current_owner_id });
    }
    if current_lease_expires_at != payload.expected_lease_expires_at {
        return Ok(ExecutionOutcome::ClaimLeaseChanged {
            current_lease_expires_at,
        });
    }
    transaction
        .execute(
            "UPDATE delegated_claims SET lease_expires_at = $4 \
             WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
            &[&command.tenant_id, &command.repository_id, &task_id, &now],
        )
        .await?;
    // Its own tag (5), distinct from claim_store::lease's 1-4 (Granted,
    // Rejected, released, not-released): the same history table records
    // decisions from two different callers, and outcome is SMALLINT, not
    // TEXT -- a string literal here fails at insert time, not compile time.
    const RELEASED_VIA_WORK_COMMAND: i16 = 5;
    transaction
        .execute(
            "INSERT INTO delegated_claim_history (tenant_id, repository_id, task_id, \
                 requested_owner_id, granted_owner_id, outcome, claim_started_at, \
                 lease_expires_at, claim_lapses, paths, symbols) \
             VALUES ($1,$2,$3,$4,$4,$6,$5,$5,0,ARRAY[]::text[],ARRAY[]::text[])",
            &[
                &command.tenant_id,
                &command.repository_id,
                &task_id,
                &current_owner_id,
                &now,
                &RELEASED_VIA_WORK_COMMAND,
            ],
        )
        .await?;
    let current = super::lock_task(
        transaction,
        &command.tenant_id,
        &command.repository_id,
        task_id,
    )
    .await?
    .ok_or(WorkCommandStoreError::UnknownCommand {
        tenant_id: command.tenant_id.clone(),
        repository_id: command.repository_id.clone(),
        command_id: command.command_id.clone(),
    })?;
    const OPEN: i16 = 1;
    const CLAIMED: i16 = 2;
    let next_state = if current.state == CLAIMED {
        OPEN
    } else {
        current.state
    };
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
    super::append_task_event(
        transaction,
        command,
        task_id,
        3,
        current.state,
        next_state,
        now,
    )
    .await?;
    Ok(ExecutionOutcome::Applied)
}

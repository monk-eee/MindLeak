//! Atomic live-delegation checks and immutable receipt writes.

use std::time::SystemTime;

use tokio_postgres::{Client, Transaction};

use super::super::{
    model::{action_codes, normalize_timestamp},
    replay::projection_from_transaction,
    DelegationProjection, DelegationProjectionStatus,
};
use super::model::{
    request_digest, row_to_use_receipt, validate_request, DelegationUseError, DelegationUseOutcome,
    DelegationUseReceipt, DelegationUseRefusal, DelegationUseRequest, USE_RECEIPT_COLUMNS,
};

pub(super) async fn authorize_use(
    client: &mut Client,
    request: DelegationUseRequest,
    now: SystemTime,
) -> Result<DelegationUseOutcome, DelegationUseError> {
    validate_request(&request)?;
    let payload_digest = request_digest(&request)?;
    let now = normalize_timestamp(now);
    let transaction = client.transaction().await?;

    if let Some(existing) = idempotent_receipt(&transaction, &request, &payload_digest).await? {
        transaction.commit().await?;
        return Ok(DelegationUseOutcome {
            receipt: existing,
            idempotent_replay: true,
        });
    }

    let projection = projection_from_transaction(
        &transaction,
        &request.tenant_id,
        &request.repository_id,
        &request.delegation_id,
        true,
    )
    .await?
    .ok_or(DelegationUseError::NotFound)?;
    let (authorized_actions, reserved_tokens) = authorized_usage(&transaction, &request).await?;
    let refusal = refusal_for(
        &projection,
        &request,
        now,
        authorized_actions,
        reserved_tokens,
    );
    let receipt = insert_receipt(
        &transaction,
        &request,
        &projection,
        refusal,
        &payload_digest,
    )
    .await?;
    transaction.commit().await?;

    Ok(DelegationUseOutcome {
        receipt,
        idempotent_replay: false,
    })
}

async fn idempotent_receipt(
    transaction: &Transaction<'_>,
    request: &DelegationUseRequest,
    payload_digest: &[u8],
) -> Result<Option<DelegationUseReceipt>, DelegationUseError> {
    let row = transaction
        .query_opt(
            &format!(
                "SELECT {USE_RECEIPT_COLUMNS} FROM delegation_use_receipts \
                 WHERE tenant_id = $1 AND repository_id = $2 AND delegation_id = $3 \
                   AND idempotency_key = $4 FOR UPDATE"
            ),
            &[
                &request.tenant_id,
                &request.repository_id,
                &request.delegation_id,
                &request.idempotency_key,
            ],
        )
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let stored_digest: Vec<u8> = row
        .try_get("payload_digest")
        .map_err(|_| DelegationUseError::InvalidStoredReceipt)?;
    if stored_digest != payload_digest {
        return Err(DelegationUseError::IdempotencyConflict);
    }
    row_to_use_receipt(&row).map(Some)
}

async fn authorized_usage(
    transaction: &Transaction<'_>,
    request: &DelegationUseRequest,
) -> Result<(u64, u64), DelegationUseError> {
    let row = transaction
        .query_one(
            "SELECT COUNT(*) FILTER (WHERE outcome = 1) AS authorized_actions, \
                    COALESCE(SUM(reserved_token_budget) FILTER (WHERE outcome = 1), 0)::bigint \
                        AS reserved_tokens \
             FROM delegation_use_receipts \
             WHERE tenant_id = $1 AND repository_id = $2 AND delegation_id = $3",
            &[
                &request.tenant_id,
                &request.repository_id,
                &request.delegation_id,
            ],
        )
        .await?;
    let actions: i64 = row
        .try_get("authorized_actions")
        .map_err(|_| DelegationUseError::InvalidStoredReceipt)?;
    let tokens: i64 = row
        .try_get("reserved_tokens")
        .map_err(|_| DelegationUseError::InvalidStoredReceipt)?;
    Ok((
        u64::try_from(actions).map_err(|_| DelegationUseError::InvalidStoredReceipt)?,
        u64::try_from(tokens).map_err(|_| DelegationUseError::InvalidStoredReceipt)?,
    ))
}

fn refusal_for(
    projection: &DelegationProjection,
    request: &DelegationUseRequest,
    now: SystemTime,
    authorized_actions: u64,
    reserved_tokens: u64,
) -> Option<DelegationUseRefusal> {
    if projection.status == DelegationProjectionStatus::Revoked {
        return Some(DelegationUseRefusal::Revoked);
    }
    if now < projection.effective_at {
        return Some(DelegationUseRefusal::NotYetEffective);
    }
    if now >= projection.expires_at {
        return Some(DelegationUseRefusal::Expired);
    }
    if projection.delegatee_session_id != request.delegatee_session_id {
        return Some(DelegationUseRefusal::DelegateeSessionMismatch);
    }
    if projection.goal_id != request.goal_id
        || !bound_scope_matches(&projection.project_id, &request.project_id)
        || !bound_scope_matches(&projection.task_id, &request.task_id)
    {
        return Some(DelegationUseRefusal::ScopeMismatch);
    }
    if projection.policy_version != request.policy_version
        || projection.policy_digest != request.policy_digest
    {
        return Some(DelegationUseRefusal::PolicyBasisMismatch);
    }
    if projection.constitution_version != request.constitution_version
        || projection.constitution_digest != request.constitution_digest
    {
        return Some(DelegationUseRefusal::ConstitutionBasisMismatch);
    }
    if !projection.allowed_actions.contains(&request.action) {
        return Some(DelegationUseRefusal::ActionNotAllowed);
    }
    if authorized_actions >= u64::from(projection.max_actions_per_session) {
        return Some(DelegationUseRefusal::ActionLimitExceeded);
    }
    if reserved_tokens
        .checked_add(u64::from(request.reserved_token_budget))
        .is_none_or(|total| total > u64::from(projection.max_token_budget))
    {
        return Some(DelegationUseRefusal::TokenBudgetExceeded);
    }
    None
}

fn bound_scope_matches(expected: &Option<String>, actual: &Option<String>) -> bool {
    expected
        .as_ref()
        .is_none_or(|expected| actual.as_ref() == Some(expected))
}

async fn insert_receipt(
    transaction: &Transaction<'_>,
    request: &DelegationUseRequest,
    projection: &DelegationProjection,
    refusal: Option<DelegationUseRefusal>,
    payload_digest: &[u8],
) -> Result<DelegationUseReceipt, DelegationUseError> {
    let action_code = action_codes(&[request.action])?
        .into_iter()
        .next()
        .ok_or(DelegationUseError::InvalidStoredReceipt)?;
    let outcome = if refusal.is_some() { 2_i16 } else { 1_i16 };
    let refusal_reason = refusal.map(super::model::refusal_reason_code);
    let delegation_version =
        i32::try_from(projection.version).map_err(|_| DelegationUseError::InvalidStoredReceipt)?;
    let row = transaction
        .query_one(
            &format!(
                "INSERT INTO delegation_use_receipts \
                 (tenant_id, repository_id, delegation_id, issuer_principal_id, \
                  delegatee_session_id, project_id, task_id, goal_id, policy_version, \
                  constitution_version, delegated_action, reserved_token_budget, \
                  delegation_version, outcome, refusal_reason, idempotency_key, payload_digest) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17) \
                 RETURNING {USE_RECEIPT_COLUMNS}"
            ),
            &[
                &request.tenant_id,
                &request.repository_id,
                &request.delegation_id,
                &projection.issuer_principal_id,
                &request.delegatee_session_id,
                &request.project_id,
                &request.task_id,
                &request.goal_id,
                &projection.policy_version,
                &projection.constitution_version,
                &action_code,
                &i64::from(request.reserved_token_budget),
                &delegation_version,
                &outcome,
                &refusal_reason,
                &request.idempotency_key,
                &payload_digest,
            ],
        )
        .await?;
    row_to_use_receipt(&row)
}

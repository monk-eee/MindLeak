//! Typed delegation-use values, validation, and durable receipt decoding.

use std::time::SystemTime;

use ackplane_protocol::delegation::DelegatedAction;
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio_postgres::Row;

use super::super::{
    model::{action_codes, actions_from_codes},
    DelegationStoreError,
};

pub(super) const MAX_RECEIPT_PAGE: i64 = 100;
pub(super) const USE_RECEIPT_COLUMNS: &str =
    "receipt_id, delegation_id, issuer_principal_id, delegatee_session_id, project_id, task_id, \
    goal_id, policy_version, constitution_version, delegated_action, reserved_token_budget, \
    delegation_version, outcome, refusal_reason, payload_digest, recorded_at";

const DIGEST_BYTES: usize = 32;
const MAX_IDENTIFIER_BYTES: usize = 256;

/// A checked request to use one named delegation for one routine operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationUseRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub delegation_id: String,
    /// The authenticated agent/supervisor session attempting the routine action.
    pub delegatee_session_id: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub goal_id: String,
    pub policy_version: String,
    pub policy_digest: Vec<u8>,
    pub constitution_version: String,
    pub constitution_digest: Vec<u8>,
    pub action: DelegatedAction,
    /// A bounded reservation against the delegation's aggregate token ceiling.
    pub reserved_token_budget: u32,
    pub idempotency_key: String,
}

/// One immutable live-delegation decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationUseReceipt {
    pub receipt_id: u64,
    pub delegation_id: String,
    pub issuer_principal_id: String,
    pub delegatee_session_id: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub goal_id: String,
    pub policy_version: String,
    pub constitution_version: String,
    pub action: DelegatedAction,
    pub reserved_token_budget: u32,
    pub delegation_version: u32,
    pub status: DelegationUseStatus,
    pub refusal_reason: Option<DelegationUseRefusal>,
    pub recorded_at: SystemTime,
}

/// Whether a requested delegation use was authorized or safely refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationUseStatus {
    Authorized,
    Refused,
}

/// Why a named delegation could not authorize a routine use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationUseRefusal {
    DelegateeSessionMismatch,
    ScopeMismatch,
    PolicyBasisMismatch,
    ConstitutionBasisMismatch,
    NotYetEffective,
    Expired,
    Revoked,
    ActionNotAllowed,
    ActionLimitExceeded,
    TokenBudgetExceeded,
}

/// The result of evaluating one request, including an exact idempotent replay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationUseOutcome {
    pub receipt: DelegationUseReceipt,
    pub idempotent_replay: bool,
}

/// A keyset boundary for delegation-use receipts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationUseReceiptCursor {
    pub receipt_id: u64,
}

/// One bounded page of immutable delegation-use receipts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationUseReceiptPage {
    pub entries: Vec<DelegationUseReceipt>,
    pub effective_limit: i64,
    pub next_after: Option<DelegationUseReceiptCursor>,
}

/// A typed failure while validating or reading a delegation-use decision.
#[derive(Debug, Error)]
pub enum DelegationUseError {
    #[error("delegation use database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("delegation use could not obtain a database connection: {0}")]
    PoolExhausted(#[from] deadpool_postgres::PoolError),
    #[error("delegation use references an unknown delegation")]
    NotFound,
    #[error("{field} must be a bounded non-empty identifier")]
    InvalidIdentifier { field: &'static str },
    #[error("{field} must be absent or a bounded non-empty identifier")]
    InvalidOptionalIdentifier { field: &'static str },
    #[error("{field} must be exactly {DIGEST_BYTES} bytes")]
    InvalidDigest { field: &'static str },
    #[error("delegation-use idempotency key was replayed with different content")]
    IdempotencyConflict,
    #[error("delegation-use receipt pagination cursor is invalid")]
    InvalidCursor,
    #[error("stored delegation-use receipt is invalid")]
    InvalidStoredReceipt,
    #[error("delegation projection is invalid: {0}")]
    Projection(#[from] DelegationStoreError),
}

pub(super) fn validate_request(request: &DelegationUseRequest) -> Result<(), DelegationUseError> {
    for (field, value) in [
        ("tenant_id", request.tenant_id.as_str()),
        ("repository_id", request.repository_id.as_str()),
        ("delegation_id", request.delegation_id.as_str()),
        (
            "delegatee_session_id",
            request.delegatee_session_id.as_str(),
        ),
        ("goal_id", request.goal_id.as_str()),
        ("policy_version", request.policy_version.as_str()),
        (
            "constitution_version",
            request.constitution_version.as_str(),
        ),
        ("idempotency_key", request.idempotency_key.as_str()),
    ] {
        require_identifier(field, value)?;
    }
    for (field, value) in [
        ("project_id", request.project_id.as_deref()),
        ("task_id", request.task_id.as_deref()),
    ] {
        validate_optional_identifier(field, value)?;
    }
    for (field, digest) in [
        ("policy_digest", request.policy_digest.as_slice()),
        (
            "constitution_digest",
            request.constitution_digest.as_slice(),
        ),
    ] {
        if digest.len() != DIGEST_BYTES {
            return Err(DelegationUseError::InvalidDigest { field });
        }
    }
    let action_codes = action_codes(&[request.action])?;
    if action_codes.len() != 1 {
        return Err(DelegationUseError::InvalidStoredReceipt);
    }
    Ok(())
}

pub(super) fn request_digest(
    request: &DelegationUseRequest,
) -> Result<Vec<u8>, DelegationUseError> {
    let action_code = action_codes(&[request.action])?
        .into_iter()
        .next()
        .ok_or(DelegationUseError::InvalidStoredReceipt)?;
    let mut hasher = Sha256::new();
    push_field(&mut hasher, b"mindleak.ackplane.v1.delegation.use\0");
    for field in [
        request.tenant_id.as_bytes(),
        request.repository_id.as_bytes(),
        request.delegation_id.as_bytes(),
        request.delegatee_session_id.as_bytes(),
        request.project_id.as_deref().unwrap_or_default().as_bytes(),
        request.task_id.as_deref().unwrap_or_default().as_bytes(),
        request.goal_id.as_bytes(),
        request.policy_version.as_bytes(),
        request.policy_digest.as_slice(),
        request.constitution_version.as_bytes(),
        request.constitution_digest.as_slice(),
        request.idempotency_key.as_bytes(),
    ] {
        push_field(&mut hasher, field);
    }
    hasher.update(action_code.to_be_bytes());
    hasher.update(request.reserved_token_budget.to_be_bytes());
    Ok(hasher.finalize().to_vec())
}

pub(super) fn row_to_use_receipt(row: &Row) -> Result<DelegationUseReceipt, DelegationUseError> {
    let action_code: i16 = row
        .try_get("delegated_action")
        .map_err(|_| DelegationUseError::InvalidStoredReceipt)?;
    let action = actions_from_codes(vec![action_code])?
        .into_iter()
        .next()
        .ok_or(DelegationUseError::InvalidStoredReceipt)?;
    let outcome: i16 = row
        .try_get("outcome")
        .map_err(|_| DelegationUseError::InvalidStoredReceipt)?;
    let refusal_reason: Option<i16> = row
        .try_get("refusal_reason")
        .map_err(|_| DelegationUseError::InvalidStoredReceipt)?;
    let status = status_from_code(outcome)?;
    let refusal_reason = match (status, refusal_reason) {
        (DelegationUseStatus::Authorized, None) => None,
        (DelegationUseStatus::Refused, Some(reason)) => Some(refusal_from_code(reason)?),
        _ => return Err(DelegationUseError::InvalidStoredReceipt),
    };
    let receipt_id: i64 = row
        .try_get("receipt_id")
        .map_err(|_| DelegationUseError::InvalidStoredReceipt)?;
    let reserved_token_budget: i64 = row
        .try_get("reserved_token_budget")
        .map_err(|_| DelegationUseError::InvalidStoredReceipt)?;
    let delegation_version: i32 = row
        .try_get("delegation_version")
        .map_err(|_| DelegationUseError::InvalidStoredReceipt)?;
    Ok(DelegationUseReceipt {
        receipt_id: u64::try_from(receipt_id)
            .map_err(|_| DelegationUseError::InvalidStoredReceipt)?,
        delegation_id: row
            .try_get("delegation_id")
            .map_err(|_| DelegationUseError::InvalidStoredReceipt)?,
        issuer_principal_id: row
            .try_get("issuer_principal_id")
            .map_err(|_| DelegationUseError::InvalidStoredReceipt)?,
        delegatee_session_id: row
            .try_get("delegatee_session_id")
            .map_err(|_| DelegationUseError::InvalidStoredReceipt)?,
        project_id: row
            .try_get("project_id")
            .map_err(|_| DelegationUseError::InvalidStoredReceipt)?,
        task_id: row
            .try_get("task_id")
            .map_err(|_| DelegationUseError::InvalidStoredReceipt)?,
        goal_id: row
            .try_get("goal_id")
            .map_err(|_| DelegationUseError::InvalidStoredReceipt)?,
        policy_version: row
            .try_get("policy_version")
            .map_err(|_| DelegationUseError::InvalidStoredReceipt)?,
        constitution_version: row
            .try_get("constitution_version")
            .map_err(|_| DelegationUseError::InvalidStoredReceipt)?,
        action,
        reserved_token_budget: u32::try_from(reserved_token_budget)
            .map_err(|_| DelegationUseError::InvalidStoredReceipt)?,
        delegation_version: u32::try_from(delegation_version)
            .map_err(|_| DelegationUseError::InvalidStoredReceipt)?,
        status,
        refusal_reason,
        recorded_at: row
            .try_get("recorded_at")
            .map_err(|_| DelegationUseError::InvalidStoredReceipt)?,
    })
}

pub(super) fn refusal_reason_code(reason: DelegationUseRefusal) -> i16 {
    match reason {
        DelegationUseRefusal::DelegateeSessionMismatch => 1,
        DelegationUseRefusal::ScopeMismatch => 2,
        DelegationUseRefusal::PolicyBasisMismatch => 3,
        DelegationUseRefusal::ConstitutionBasisMismatch => 4,
        DelegationUseRefusal::NotYetEffective => 5,
        DelegationUseRefusal::Expired => 6,
        DelegationUseRefusal::Revoked => 7,
        DelegationUseRefusal::ActionNotAllowed => 8,
        DelegationUseRefusal::ActionLimitExceeded => 9,
        DelegationUseRefusal::TokenBudgetExceeded => 10,
    }
}

fn status_from_code(value: i16) -> Result<DelegationUseStatus, DelegationUseError> {
    match value {
        1 => Ok(DelegationUseStatus::Authorized),
        2 => Ok(DelegationUseStatus::Refused),
        _ => Err(DelegationUseError::InvalidStoredReceipt),
    }
}

fn refusal_from_code(value: i16) -> Result<DelegationUseRefusal, DelegationUseError> {
    match value {
        1 => Ok(DelegationUseRefusal::DelegateeSessionMismatch),
        2 => Ok(DelegationUseRefusal::ScopeMismatch),
        3 => Ok(DelegationUseRefusal::PolicyBasisMismatch),
        4 => Ok(DelegationUseRefusal::ConstitutionBasisMismatch),
        5 => Ok(DelegationUseRefusal::NotYetEffective),
        6 => Ok(DelegationUseRefusal::Expired),
        7 => Ok(DelegationUseRefusal::Revoked),
        8 => Ok(DelegationUseRefusal::ActionNotAllowed),
        9 => Ok(DelegationUseRefusal::ActionLimitExceeded),
        10 => Ok(DelegationUseRefusal::TokenBudgetExceeded),
        _ => Err(DelegationUseError::InvalidStoredReceipt),
    }
}

fn require_identifier(field: &'static str, value: &str) -> Result<(), DelegationUseError> {
    if value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES {
        return Err(DelegationUseError::InvalidIdentifier { field });
    }
    Ok(())
}

fn validate_optional_identifier(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), DelegationUseError> {
    if value.is_some_and(|value| value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES) {
        return Err(DelegationUseError::InvalidOptionalIdentifier { field });
    }
    Ok(())
}

fn push_field(hasher: &mut Sha256, field: &[u8]) {
    hasher.update((field.len() as u32).to_be_bytes());
    hasher.update(field);
}

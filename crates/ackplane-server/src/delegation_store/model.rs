//! Bounded value objects and canonical payload digests for ADR-0115 grants.

use std::{collections::HashSet, time::SystemTime};

use ackplane_protocol::delegation::DelegatedAction;
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::OffsetDateTime;

pub(super) const SHA256_DIGEST_BYTES: usize = 32;
const MAX_IDENTIFIER_BYTES: usize = 256;
const MAX_REASON_BYTES: usize = 512;
const MAX_ACTIONS: usize = 16;
const EVENT_SCHEMA_VERSION: u16 = 1;

mod row;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationGrantRequest {
    pub tenant_id: String,
    pub repository_id: String,
    /// The caller must have verified this principal before calling the store.
    pub verified_issuer_principal_id: String,
    pub delegatee_session_id: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub goal_id: String,
    pub goal_digest: Vec<u8>,
    pub policy_version: String,
    pub policy_digest: Vec<u8>,
    pub constitution_version: String,
    pub constitution_digest: Vec<u8>,
    pub allowed_actions: Vec<DelegatedAction>,
    pub max_token_budget: u32,
    pub max_actions_per_session: u32,
    pub source_protocol_version: u16,
    pub effective_at: SystemTime,
    pub expires_at: SystemTime,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationRevocationRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub delegation_id: String,
    /// The caller must have verified this principal before calling the store.
    pub verified_revoker_principal_id: String,
    pub reason: String,
    pub expected_version: u32,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationEventKind {
    Granted,
    Revoked,
}

impl DelegationEventKind {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::Granted => 1,
            Self::Revoked => 2,
        }
    }

    fn from_i16(value: i16) -> Option<Self> {
        match value {
            1 => Some(Self::Granted),
            2 => Some(Self::Revoked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationProjectionStatus {
    Active,
    Revoked,
}

impl DelegationProjectionStatus {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::Active => 1,
            Self::Revoked => 2,
        }
    }

    fn from_i16(value: i16) -> Option<Self> {
        match value {
            1 => Some(Self::Active),
            2 => Some(Self::Revoked),
            _ => None,
        }
    }
}

/// The bounded, operation-specific content retained by an immutable event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DelegationEventPayload {
    Granted(Box<DelegationGrantPayload>),
    Revoked { reason: String },
}

/// The complete bounded grant envelope required to reproduce a projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationGrantPayload {
    pub delegatee_session_id: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub goal_id: String,
    pub goal_digest: Vec<u8>,
    pub policy_version: String,
    pub policy_digest: Vec<u8>,
    pub constitution_version: String,
    pub constitution_digest: Vec<u8>,
    pub allowed_actions: Vec<DelegatedAction>,
    pub max_token_budget: u32,
    pub max_actions_per_session: u32,
    pub source_protocol_version: u16,
    pub effective_at: SystemTime,
    pub expires_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationEvent {
    pub delegation_id: String,
    pub stream_position: u64,
    pub kind: DelegationEventKind,
    pub actor_principal_id: String,
    pub expected_prior_version: u32,
    pub resulting_version: u32,
    pub idempotency_key: String,
    pub payload_digest: Vec<u8>,
    pub schema_version: u16,
    pub recorded_at: SystemTime,
    pub payload: DelegationEventPayload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationProjection {
    pub delegation_id: String,
    pub issuer_principal_id: String,
    pub delegatee_session_id: String,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub goal_id: String,
    pub goal_digest: Vec<u8>,
    pub policy_version: String,
    pub policy_digest: Vec<u8>,
    pub constitution_version: String,
    pub constitution_digest: Vec<u8>,
    pub allowed_actions: Vec<DelegatedAction>,
    pub max_token_budget: u32,
    pub max_actions_per_session: u32,
    pub source_protocol_version: u16,
    pub issued_at: SystemTime,
    pub effective_at: SystemTime,
    pub expires_at: SystemTime,
    pub status: DelegationProjectionStatus,
    pub version: u32,
    pub source_event_position: u64,
    pub revoked_at: Option<SystemTime>,
    pub revoked_by_principal_id: Option<String>,
    pub revocation_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationOutcome {
    pub projection: DelegationProjection,
    pub event: DelegationEvent,
    pub idempotent_replay: bool,
}

#[derive(Debug, Error)]
pub enum DelegationStoreError {
    #[error("delegation database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("delegation could not obtain a database connection: {0}")]
    PoolExhausted(#[from] deadpool_postgres::PoolError),
    #[error("delegation identity entropy failed: {0}")]
    Entropy(#[from] getrandom::Error),
    #[error("{field} must be a bounded non-empty identifier")]
    InvalidIdentifier { field: &'static str },
    #[error("{field} must be absent or a bounded non-empty identifier")]
    InvalidOptionalIdentifier { field: &'static str },
    #[error("{field} must be exactly {SHA256_DIGEST_BYTES} bytes")]
    InvalidDigest { field: &'static str },
    #[error("delegation must include between 1 and {MAX_ACTIONS} distinct routine actions")]
    InvalidActions,
    #[error("delegated action code {0} is outside the routine vocabulary")]
    InvalidStoredAction(i16),
    #[error("delegation budgets must be greater than zero")]
    InvalidBudget,
    #[error("source_protocol_version must be greater than zero")]
    InvalidProtocolVersion,
    #[error("delegation expiry must follow effectivity and remain in the future")]
    InvalidTimeWindow,
    #[error("expected_version must be greater than zero")]
    InvalidExpectedVersion,
    #[error("delegation version exceeds the supported projection range")]
    InvalidVersion,
    #[error("delegation event stream position is exhausted")]
    StreamPositionExhausted,
    #[error("delegation idempotency identity was already used for a different operation")]
    IdempotencyConflict,
    #[error("delegation was not found in this tenant and repository")]
    NotFound,
    #[error("delegation version did not match the current projection")]
    VersionConflict,
    #[error("delegation is already revoked")]
    AlreadyRevoked,
    #[error("stored delegation event kind {0} is invalid")]
    InvalidStoredEventKind(i16),
    #[error("stored delegation projection status {0} is invalid")]
    InvalidStoredStatus(i16),
    #[error("stored delegation event payload is incomplete or inconsistent")]
    InvalidStoredPayload,
    #[error("stored delegation numeric field {field} is invalid")]
    InvalidStoredNumber { field: &'static str },
}

pub(super) fn normalize_timestamp(timestamp: SystemTime) -> SystemTime {
    let timestamp = OffsetDateTime::from(timestamp);
    let remainder = timestamp.nanosecond() % 1_000;
    (timestamp - time::Duration::nanoseconds(i64::from(remainder))).into()
}

pub(super) fn validate_grant(
    request: &DelegationGrantRequest,
    now: SystemTime,
) -> Result<(), DelegationStoreError> {
    for (field, value) in [
        ("tenant_id", request.tenant_id.as_str()),
        ("repository_id", request.repository_id.as_str()),
        (
            "verified_issuer_principal_id",
            request.verified_issuer_principal_id.as_str(),
        ),
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
        ("goal_digest", request.goal_digest.as_slice()),
        ("policy_digest", request.policy_digest.as_slice()),
        (
            "constitution_digest",
            request.constitution_digest.as_slice(),
        ),
    ] {
        if digest.len() != SHA256_DIGEST_BYTES {
            return Err(DelegationStoreError::InvalidDigest { field });
        }
    }
    let _ = action_codes(&request.allowed_actions)?;
    if request.max_token_budget == 0 || request.max_actions_per_session == 0 {
        return Err(DelegationStoreError::InvalidBudget);
    }
    if request.source_protocol_version == 0 || request.source_protocol_version > i16::MAX as u16 {
        return Err(DelegationStoreError::InvalidProtocolVersion);
    }
    if request.effective_at < SystemTime::UNIX_EPOCH
        || request.expires_at <= request.effective_at
        || request.expires_at <= now
    {
        return Err(DelegationStoreError::InvalidTimeWindow);
    }
    Ok(())
}

pub(super) fn validate_revocation(
    request: &DelegationRevocationRequest,
) -> Result<(), DelegationStoreError> {
    for (field, value) in [
        ("tenant_id", request.tenant_id.as_str()),
        ("repository_id", request.repository_id.as_str()),
        ("delegation_id", request.delegation_id.as_str()),
        (
            "verified_revoker_principal_id",
            request.verified_revoker_principal_id.as_str(),
        ),
        ("idempotency_key", request.idempotency_key.as_str()),
    ] {
        require_identifier(field, value)?;
    }
    if request.reason.trim().is_empty() || request.reason.len() > MAX_REASON_BYTES {
        return Err(DelegationStoreError::InvalidIdentifier { field: "reason" });
    }
    if request.expected_version == 0 {
        return Err(DelegationStoreError::InvalidExpectedVersion);
    }
    if request.expected_version >= i32::MAX as u32 {
        return Err(DelegationStoreError::InvalidVersion);
    }
    Ok(())
}

pub(super) fn grant_payload_digest(request: &DelegationGrantRequest) -> Vec<u8> {
    let mut hasher = Sha256::new();
    push_field(&mut hasher, b"mindleak.ackplane.v1.delegation.grant\0");
    for field in [
        request.tenant_id.as_bytes(),
        request.repository_id.as_bytes(),
        request.verified_issuer_principal_id.as_bytes(),
        request.delegatee_session_id.as_bytes(),
        request.project_id.as_deref().unwrap_or_default().as_bytes(),
        request.task_id.as_deref().unwrap_or_default().as_bytes(),
        request.goal_id.as_bytes(),
        request.goal_digest.as_slice(),
        request.policy_version.as_bytes(),
        request.policy_digest.as_slice(),
        request.constitution_version.as_bytes(),
        request.constitution_digest.as_slice(),
    ] {
        push_field(&mut hasher, field);
    }
    let actions = action_codes(&request.allowed_actions).expect("validated grant actions");
    hasher.update((actions.len() as u32).to_be_bytes());
    for action in actions {
        hasher.update(action.to_be_bytes());
    }
    hasher.update(request.max_token_budget.to_be_bytes());
    hasher.update(request.max_actions_per_session.to_be_bytes());
    hasher.update(request.source_protocol_version.to_be_bytes());
    push_field(&mut hasher, &unix_nanos(request.effective_at).to_be_bytes());
    push_field(&mut hasher, &unix_nanos(request.expires_at).to_be_bytes());
    hasher.finalize().to_vec()
}

pub(super) fn revocation_payload_digest(request: &DelegationRevocationRequest) -> Vec<u8> {
    let mut hasher = Sha256::new();
    push_field(&mut hasher, b"mindleak.ackplane.v1.delegation.revoke\0");
    for field in [
        request.tenant_id.as_bytes(),
        request.repository_id.as_bytes(),
        request.delegation_id.as_bytes(),
        request.verified_revoker_principal_id.as_bytes(),
        request.reason.as_bytes(),
    ] {
        push_field(&mut hasher, field);
    }
    hasher.update(request.expected_version.to_be_bytes());
    hasher.finalize().to_vec()
}

pub(super) fn action_codes(actions: &[DelegatedAction]) -> Result<Vec<i16>, DelegationStoreError> {
    if actions.is_empty() || actions.len() > MAX_ACTIONS {
        return Err(DelegationStoreError::InvalidActions);
    }
    let mut seen = HashSet::new();
    let mut codes = Vec::with_capacity(actions.len());
    for action in actions {
        let code = match action {
            DelegatedAction::RetrieveContext => 1,
            DelegatedAction::Analyze => 2,
            DelegatedAction::ClaimTask => 3,
            DelegatedAction::WorkTask => 4,
            DelegatedAction::CreateCandidateKnowledge => 5,
            DelegatedAction::RunValidation => 6,
            DelegatedAction::ReportEvidence => 7,
        };
        if !seen.insert(code) {
            return Err(DelegationStoreError::InvalidActions);
        }
        codes.push(code);
    }
    codes.sort_unstable();
    Ok(codes)
}

pub(super) fn actions_from_codes(
    codes: Vec<i16>,
) -> Result<Vec<DelegatedAction>, DelegationStoreError> {
    let mut actions = Vec::with_capacity(codes.len());
    let mut seen = HashSet::new();
    for code in codes {
        let action = match code {
            1 => DelegatedAction::RetrieveContext,
            2 => DelegatedAction::Analyze,
            3 => DelegatedAction::ClaimTask,
            4 => DelegatedAction::WorkTask,
            5 => DelegatedAction::CreateCandidateKnowledge,
            6 => DelegatedAction::RunValidation,
            7 => DelegatedAction::ReportEvidence,
            _ => return Err(DelegationStoreError::InvalidStoredAction(code)),
        };
        if !seen.insert(code) {
            return Err(DelegationStoreError::InvalidStoredAction(code));
        }
        actions.push(action);
    }
    if actions.is_empty() || actions.len() > MAX_ACTIONS {
        return Err(DelegationStoreError::InvalidActions);
    }
    Ok(actions)
}

pub(super) use row::{projection_at_event, row_to_event, row_to_projection};

pub(super) fn event_schema_version() -> i16 {
    EVENT_SCHEMA_VERSION as i16
}

fn require_identifier(field: &'static str, value: &str) -> Result<(), DelegationStoreError> {
    if value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES {
        return Err(DelegationStoreError::InvalidIdentifier { field });
    }
    Ok(())
}

fn validate_optional_identifier(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), DelegationStoreError> {
    if value.is_some_and(|value| value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES) {
        return Err(DelegationStoreError::InvalidOptionalIdentifier { field });
    }
    Ok(())
}

fn push_field(hasher: &mut Sha256, field: &[u8]) {
    hasher.update((field.len() as u32).to_be_bytes());
    hasher.update(field);
}

fn unix_nanos(timestamp: SystemTime) -> u128 {
    timestamp
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("validated delegation timestamps must be after the Unix epoch")
        .as_nanos()
}

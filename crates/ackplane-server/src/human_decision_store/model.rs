//! Bounded value objects and canonical payload digests for ADR-0115's human
//! decision (escalation) requests.

use std::time::SystemTime;

use sha2::{Digest, Sha256};
use thiserror::Error;
use time::OffsetDateTime;

pub(super) const SHA256_DIGEST_BYTES: usize = 32;
const MAX_IDENTIFIER_BYTES: usize = 256;
const MAX_REASON_BYTES: usize = 512;
const MAX_ALTERNATIVES_BYTES: usize = 512;
const EVENT_SCHEMA_VERSION: u16 = 1;

mod row;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDecisionRequest {
    pub tenant_id: String,
    pub repository_id: String,
    /// The caller must have verified this principal before calling the store.
    pub verified_proposing_principal_id: String,
    pub proposed_action: String,
    pub target: String,
    pub reason: String,
    pub context_packet_digest: Vec<u8>,
    pub evidence_digest: Vec<u8>,
    pub alternatives: String,
    pub safe_behavior: SafeBehavior,
    pub related_delegation_id: Option<String>,
    pub expires_at: SystemTime,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanDecisionResolutionOutcome {
    Approved,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDecisionResolutionRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub decision_id: String,
    /// The caller must have verified this principal before calling the
    /// store. It must differ from the request's proposing principal
    /// (ADR-0115 item 8: separation of duties).
    pub verified_resolving_principal_id: String,
    pub outcome: HumanDecisionResolutionOutcome,
    pub rationale: String,
    pub expected_version: u32,
    pub idempotency_key: String,
}

/// The safe behavior an agent follows while a decision is pending
/// (ADR-0115 item 6: no response is not an approval).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafeBehavior {
    ContinueWithinDelegation,
    CheckpointAndPause,
    Drain,
    Refuse,
}

impl SafeBehavior {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::ContinueWithinDelegation => 1,
            Self::CheckpointAndPause => 2,
            Self::Drain => 3,
            Self::Refuse => 4,
        }
    }

    fn from_i16(value: i16) -> Option<Self> {
        match value {
            1 => Some(Self::ContinueWithinDelegation),
            2 => Some(Self::CheckpointAndPause),
            3 => Some(Self::Drain),
            4 => Some(Self::Refuse),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanDecisionEventKind {
    Requested,
    Approved,
    Denied,
}

impl HumanDecisionEventKind {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::Requested => 1,
            Self::Approved => 2,
            Self::Denied => 3,
        }
    }

    fn from_i16(value: i16) -> Option<Self> {
        match value {
            1 => Some(Self::Requested),
            2 => Some(Self::Approved),
            3 => Some(Self::Denied),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanDecisionStatus {
    Pending,
    Approved,
    Denied,
}

impl HumanDecisionStatus {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::Pending => 1,
            Self::Approved => 2,
            Self::Denied => 3,
        }
    }

    fn from_i16(value: i16) -> Option<Self> {
        match value {
            1 => Some(Self::Pending),
            2 => Some(Self::Approved),
            3 => Some(Self::Denied),
            _ => None,
        }
    }
}

/// The bounded, operation-specific content retained by an immutable event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HumanDecisionEventPayload {
    Requested(Box<HumanDecisionRequestedPayload>),
    Resolved {
        outcome: HumanDecisionResolutionOutcome,
        rationale: String,
    },
}

/// The complete bounded request envelope required to reproduce a projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDecisionRequestedPayload {
    pub proposed_action: String,
    pub target: String,
    pub reason: String,
    pub context_packet_digest: Vec<u8>,
    pub evidence_digest: Vec<u8>,
    pub alternatives: String,
    pub safe_behavior: SafeBehavior,
    pub related_delegation_id: Option<String>,
    pub expires_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDecisionEvent {
    pub decision_id: String,
    pub stream_position: u64,
    pub kind: HumanDecisionEventKind,
    pub actor_principal_id: String,
    pub expected_prior_version: u32,
    pub resulting_version: u32,
    pub idempotency_key: String,
    pub payload_digest: Vec<u8>,
    pub schema_version: u16,
    pub recorded_at: SystemTime,
    pub payload: HumanDecisionEventPayload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDecisionProjection {
    pub decision_id: String,
    pub proposing_principal_id: String,
    pub proposed_action: String,
    pub target: String,
    pub reason: String,
    pub context_packet_digest: Vec<u8>,
    pub evidence_digest: Vec<u8>,
    pub alternatives: String,
    pub safe_behavior: SafeBehavior,
    pub related_delegation_id: Option<String>,
    pub requested_at: SystemTime,
    pub expires_at: SystemTime,
    pub status: HumanDecisionStatus,
    pub version: u32,
    pub source_event_position: u64,
    pub resolved_at: Option<SystemTime>,
    pub resolved_by_principal_id: Option<String>,
    pub resolution_rationale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanDecisionOutcome {
    pub projection: HumanDecisionProjection,
    pub event: HumanDecisionEvent,
    pub idempotent_replay: bool,
}

#[derive(Debug, Error)]
pub enum HumanDecisionStoreError {
    #[error("human decision database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("human decision store could not obtain a database connection: {0}")]
    PoolExhausted(#[from] deadpool_postgres::PoolError),
    #[error("human decision identity entropy failed: {0}")]
    Entropy(#[from] getrandom::Error),
    #[error("{field} must be a bounded non-empty identifier")]
    InvalidIdentifier { field: &'static str },
    #[error("{field} must be absent or a bounded non-empty identifier")]
    InvalidOptionalIdentifier { field: &'static str },
    #[error("{field} must be exactly {SHA256_DIGEST_BYTES} bytes")]
    InvalidDigest { field: &'static str },
    #[error("human decision expiry must remain in the future")]
    InvalidTimeWindow,
    #[error("expected_version must be greater than zero")]
    InvalidExpectedVersion,
    #[error("human decision version exceeds the supported projection range")]
    InvalidVersion,
    #[error("human decision event stream position is exhausted")]
    StreamPositionExhausted,
    #[error("human decision idempotency identity was already used for a different operation")]
    IdempotencyConflict,
    #[error("human decision request was not found in this tenant and repository")]
    NotFound,
    #[error("human decision version did not match the current projection")]
    VersionConflict,
    #[error("human decision request is already resolved")]
    AlreadyResolved,
    #[error(
        "the resolving principal must differ from the proposing principal \
         (ADR-0115 item 8: separation of duties)"
    )]
    SeparationOfDutiesViolation,
    #[error("stored human decision event kind {0} is invalid")]
    InvalidStoredEventKind(i16),
    #[error("stored human decision projection status {0} is invalid")]
    InvalidStoredStatus(i16),
    #[error("stored human decision safe behavior {0} is invalid")]
    InvalidStoredSafeBehavior(i16),
    #[error("stored human decision event payload is incomplete or inconsistent")]
    InvalidStoredPayload,
    #[error("stored human decision numeric field {field} is invalid")]
    InvalidStoredNumber { field: &'static str },
}

pub(super) fn normalize_timestamp(timestamp: SystemTime) -> SystemTime {
    let timestamp = OffsetDateTime::from(timestamp);
    let remainder = timestamp.nanosecond() % 1_000;
    (timestamp - time::Duration::nanoseconds(i64::from(remainder))).into()
}

pub(super) fn validate_request(
    request: &HumanDecisionRequest,
    now: SystemTime,
) -> Result<(), HumanDecisionStoreError> {
    for (field, value) in [
        ("tenant_id", request.tenant_id.as_str()),
        ("repository_id", request.repository_id.as_str()),
        (
            "verified_proposing_principal_id",
            request.verified_proposing_principal_id.as_str(),
        ),
        ("proposed_action", request.proposed_action.as_str()),
        ("target", request.target.as_str()),
        ("idempotency_key", request.idempotency_key.as_str()),
    ] {
        require_identifier(field, value)?;
    }
    validate_optional_identifier(
        "related_delegation_id",
        request.related_delegation_id.as_deref(),
    )?;
    if request.reason.trim().is_empty() || request.reason.len() > MAX_REASON_BYTES {
        return Err(HumanDecisionStoreError::InvalidIdentifier { field: "reason" });
    }
    if request.alternatives.trim().is_empty() || request.alternatives.len() > MAX_ALTERNATIVES_BYTES
    {
        return Err(HumanDecisionStoreError::InvalidIdentifier {
            field: "alternatives",
        });
    }
    for (field, digest) in [
        (
            "context_packet_digest",
            request.context_packet_digest.as_slice(),
        ),
        ("evidence_digest", request.evidence_digest.as_slice()),
    ] {
        if digest.len() != SHA256_DIGEST_BYTES {
            return Err(HumanDecisionStoreError::InvalidDigest { field });
        }
    }
    if request.expires_at <= now {
        return Err(HumanDecisionStoreError::InvalidTimeWindow);
    }
    Ok(())
}

pub(super) fn validate_resolution(
    request: &HumanDecisionResolutionRequest,
) -> Result<(), HumanDecisionStoreError> {
    for (field, value) in [
        ("tenant_id", request.tenant_id.as_str()),
        ("repository_id", request.repository_id.as_str()),
        ("decision_id", request.decision_id.as_str()),
        (
            "verified_resolving_principal_id",
            request.verified_resolving_principal_id.as_str(),
        ),
        ("idempotency_key", request.idempotency_key.as_str()),
    ] {
        require_identifier(field, value)?;
    }
    if request.rationale.trim().is_empty() || request.rationale.len() > MAX_REASON_BYTES {
        return Err(HumanDecisionStoreError::InvalidIdentifier { field: "rationale" });
    }
    if request.expected_version == 0 {
        return Err(HumanDecisionStoreError::InvalidExpectedVersion);
    }
    if request.expected_version >= i32::MAX as u32 {
        return Err(HumanDecisionStoreError::InvalidVersion);
    }
    Ok(())
}

pub(super) fn request_payload_digest(request: &HumanDecisionRequest) -> Vec<u8> {
    let mut hasher = Sha256::new();
    push_field(
        &mut hasher,
        b"mindleak.ackplane.v1.human_decision.request\0",
    );
    for field in [
        request.tenant_id.as_bytes(),
        request.repository_id.as_bytes(),
        request.verified_proposing_principal_id.as_bytes(),
        request.proposed_action.as_bytes(),
        request.target.as_bytes(),
        request.reason.as_bytes(),
        request.context_packet_digest.as_slice(),
        request.evidence_digest.as_slice(),
        request.alternatives.as_bytes(),
        request
            .related_delegation_id
            .as_deref()
            .unwrap_or_default()
            .as_bytes(),
    ] {
        push_field(&mut hasher, field);
    }
    hasher.update(request.safe_behavior.as_i16().to_be_bytes());
    push_field(&mut hasher, &unix_nanos(request.expires_at).to_be_bytes());
    hasher.finalize().to_vec()
}

pub(super) fn resolution_payload_digest(request: &HumanDecisionResolutionRequest) -> Vec<u8> {
    let mut hasher = Sha256::new();
    push_field(
        &mut hasher,
        b"mindleak.ackplane.v1.human_decision.resolve\0",
    );
    for field in [
        request.tenant_id.as_bytes(),
        request.repository_id.as_bytes(),
        request.decision_id.as_bytes(),
        request.verified_resolving_principal_id.as_bytes(),
        request.rationale.as_bytes(),
    ] {
        push_field(&mut hasher, field);
    }
    let outcome_code: i16 = match request.outcome {
        HumanDecisionResolutionOutcome::Approved => 1,
        HumanDecisionResolutionOutcome::Denied => 2,
    };
    hasher.update(outcome_code.to_be_bytes());
    hasher.update(request.expected_version.to_be_bytes());
    hasher.finalize().to_vec()
}

pub(super) use row::{projection_at_event, row_to_event, row_to_projection};

pub(super) fn event_schema_version() -> i16 {
    EVENT_SCHEMA_VERSION as i16
}

fn require_identifier(field: &'static str, value: &str) -> Result<(), HumanDecisionStoreError> {
    if value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES {
        return Err(HumanDecisionStoreError::InvalidIdentifier { field });
    }
    Ok(())
}

fn validate_optional_identifier(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), HumanDecisionStoreError> {
    if value.is_some_and(|value| value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES) {
        return Err(HumanDecisionStoreError::InvalidOptionalIdentifier { field });
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
        .expect("validated human decision timestamps must be after the Unix epoch")
        .as_nanos()
}

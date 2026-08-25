use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio_postgres::Row;

const DIGEST_BYTES: usize = 32;
const MAX_IDENTIFIER_BYTES: usize = 256;
const MAX_POLICY_REFS: usize = 32;
const MAX_RATIONALE_BYTES: usize = 4_096;
const MAX_REASON_BYTES: usize = 4_096;

/// The closed vocabulary ADR-0125 permits in its first command contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkCommandKind {
    CreateWork,
    RouteWork,
    ReleaseLease,
    AnswerWait,
    SubmitReview,
    Assign,
    Steer,
    Pause,
    Resume,
    Drain,
}

impl WorkCommandKind {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::CreateWork => 1,
            Self::RouteWork => 2,
            Self::ReleaseLease => 3,
            Self::AnswerWait => 4,
            Self::SubmitReview => 5,
            Self::Assign => 6,
            Self::Steer => 7,
            Self::Pause => 8,
            Self::Resume => 9,
            Self::Drain => 10,
        }
    }

    fn from_i16(value: i16) -> Result<Self, WorkCommandStoreError> {
        match value {
            1 => Ok(Self::CreateWork),
            2 => Ok(Self::RouteWork),
            3 => Ok(Self::ReleaseLease),
            4 => Ok(Self::AnswerWait),
            5 => Ok(Self::SubmitReview),
            6 => Ok(Self::Assign),
            7 => Ok(Self::Steer),
            8 => Ok(Self::Pause),
            9 => Ok(Self::Resume),
            10 => Ok(Self::Drain),
            other => Err(WorkCommandStoreError::UnknownCommandKind { value: other }),
        }
    }
}

/// A durable command outcome. A receipt describes what happened; it does not
/// imply an asynchronous supervisor effect has completed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkCommandOutcome {
    PendingConfirmation,
    PendingDelivery,
    Accepted,
    Applied,
    Failed,
    Expired,
    Conflicted,
    Refused,
}

impl WorkCommandOutcome {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::PendingConfirmation => 1,
            Self::PendingDelivery => 2,
            Self::Accepted => 3,
            Self::Applied => 4,
            Self::Failed => 5,
            Self::Expired => 6,
            Self::Conflicted => 7,
            Self::Refused => 8,
        }
    }

    fn from_i16(value: i16) -> Result<Self, WorkCommandStoreError> {
        match value {
            1 => Ok(Self::PendingConfirmation),
            2 => Ok(Self::PendingDelivery),
            3 => Ok(Self::Accepted),
            4 => Ok(Self::Applied),
            5 => Ok(Self::Failed),
            6 => Ok(Self::Expired),
            7 => Ok(Self::Conflicted),
            8 => Ok(Self::Refused),
            other => Err(WorkCommandStoreError::UnknownOutcome { value: other }),
        }
    }
}

/// An immutable command request after the command service has allocated its
/// id and canonical payload digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWorkCommand {
    pub tenant_id: String,
    pub repository_id: String,
    pub command_id: String,
    pub kind: WorkCommandKind,
    pub schema_version: String,
    pub task_id: Option<String>,
    pub issuing_principal_id: String,
    pub delegation_id: Option<String>,
    pub policy_refs: Vec<String>,
    pub rationale: String,
    pub expected_task_version: Option<i64>,
    pub confirmation_id: Option<String>,
    pub expires_at: SystemTime,
    pub idempotency_key: String,
    pub payload_digest: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkCommand {
    pub tenant_id: String,
    pub repository_id: String,
    pub command_id: String,
    pub kind: WorkCommandKind,
    pub schema_version: String,
    pub task_id: Option<String>,
    pub issuing_principal_id: String,
    pub delegation_id: Option<String>,
    pub policy_refs: Vec<String>,
    pub rationale: String,
    pub expected_task_version: Option<i64>,
    pub confirmation_id: Option<String>,
    pub expires_at: SystemTime,
    pub idempotency_key: String,
    pub request_digest: Vec<u8>,
    pub payload_digest: Vec<u8>,
    pub recorded_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWorkCommandReceipt {
    pub tenant_id: String,
    pub repository_id: String,
    pub command_id: String,
    pub receipt_id: String,
    pub outcome: WorkCommandOutcome,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub occurred_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkCommandReceipt {
    pub tenant_id: String,
    pub repository_id: String,
    pub command_id: String,
    pub receipt_id: String,
    pub outcome: WorkCommandOutcome,
    pub reason: String,
    pub evidence_refs: Vec<String>,
    pub receipt_digest: Vec<u8>,
    pub occurred_at: SystemTime,
    pub recorded_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkCommandWriteOutcome {
    pub command: WorkCommand,
    pub idempotent_replay: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkCommandReceiptWriteOutcome {
    pub receipt: WorkCommandReceipt,
    pub idempotent_replay: bool,
}

#[derive(Debug, Error)]
pub enum WorkCommandStoreError {
    #[error("work command database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("{field} must be a bounded non-empty identifier")]
    InvalidIdentifier { field: &'static str },
    #[error("{field} must be absent or a bounded non-empty identifier")]
    InvalidOptionalIdentifier { field: &'static str },
    #[error("policy_refs must contain at most {MAX_POLICY_REFS} bounded identifiers")]
    InvalidPolicyReferences,
    #[error("rationale must be between 1 and {MAX_RATIONALE_BYTES} bytes")]
    InvalidRationale,
    #[error("receipt reason must be at most {MAX_REASON_BYTES} bytes")]
    InvalidReason,
    #[error("payload_digest must be exactly {DIGEST_BYTES} bytes")]
    InvalidPayloadDigest,
    #[error("expected task version must be non-negative")]
    InvalidExpectedTaskVersion,
    #[error("CreateWork must not name an existing task or expected task version")]
    InvalidCreateWorkTarget,
    #[error("an existing Work command requires task_id and expected_task_version")]
    MissingExistingTaskVersion,
    #[error("command expiry must be after the request time")]
    InvalidExpiry,
    #[error("receipt time must not be in the future")]
    InvalidReceiptTime,
    #[error("timestamp must be at or after the Unix epoch")]
    InvalidTimestamp,
    #[error("unknown Work command kind: {value}")]
    UnknownCommandKind { value: i16 },
    #[error("unknown Work command outcome: {value}")]
    UnknownOutcome { value: i16 },
    #[error("unknown Work command {tenant_id}/{repository_id}/{command_id}")]
    UnknownCommand {
        tenant_id: String,
        repository_id: String,
        command_id: String,
    },
    #[error("command id or idempotency key was replayed with different content")]
    IdempotencyConflict,
    #[error("receipt id was replayed with different content")]
    ReceiptConflict,
}

pub(super) fn validate_request(
    request: &NewWorkCommand,
    now: SystemTime,
) -> Result<(), WorkCommandStoreError> {
    for (field, value) in [
        ("tenant_id", request.tenant_id.as_str()),
        ("repository_id", request.repository_id.as_str()),
        ("command_id", request.command_id.as_str()),
        ("schema_version", request.schema_version.as_str()),
        (
            "issuing_principal_id",
            request.issuing_principal_id.as_str(),
        ),
        ("idempotency_key", request.idempotency_key.as_str()),
    ] {
        require_identifier(field, value)?;
    }
    for (field, value) in [
        ("task_id", request.task_id.as_deref()),
        ("delegation_id", request.delegation_id.as_deref()),
        ("confirmation_id", request.confirmation_id.as_deref()),
    ] {
        validate_optional_identifier(field, value)?;
    }
    if request.policy_refs.len() > MAX_POLICY_REFS
        || request
            .policy_refs
            .iter()
            .any(|reference| !is_identifier(reference))
    {
        return Err(WorkCommandStoreError::InvalidPolicyReferences);
    }
    if request.rationale.is_empty() || request.rationale.len() > MAX_RATIONALE_BYTES {
        return Err(WorkCommandStoreError::InvalidRationale);
    }
    if request.payload_digest.len() != DIGEST_BYTES {
        return Err(WorkCommandStoreError::InvalidPayloadDigest);
    }
    if request
        .expected_task_version
        .is_some_and(|version| version < 0)
    {
        return Err(WorkCommandStoreError::InvalidExpectedTaskVersion);
    }
    match request.kind {
        WorkCommandKind::CreateWork
            if request.task_id.is_some() || request.expected_task_version.is_some() =>
        {
            return Err(WorkCommandStoreError::InvalidCreateWorkTarget);
        }
        WorkCommandKind::CreateWork => {}
        _ if request.task_id.is_none() || request.expected_task_version.is_none() => {
            return Err(WorkCommandStoreError::MissingExistingTaskVersion);
        }
        _ => {}
    }
    if request.expires_at <= now {
        return Err(WorkCommandStoreError::InvalidExpiry);
    }
    Ok(())
}

pub(super) fn validate_receipt(
    receipt: &NewWorkCommandReceipt,
    now: SystemTime,
) -> Result<(), WorkCommandStoreError> {
    for (field, value) in [
        ("tenant_id", receipt.tenant_id.as_str()),
        ("repository_id", receipt.repository_id.as_str()),
        ("command_id", receipt.command_id.as_str()),
        ("receipt_id", receipt.receipt_id.as_str()),
    ] {
        require_identifier(field, value)?;
    }
    if receipt.reason.len() > MAX_REASON_BYTES {
        return Err(WorkCommandStoreError::InvalidReason);
    }
    if receipt.evidence_refs.len() > MAX_POLICY_REFS
        || receipt
            .evidence_refs
            .iter()
            .any(|reference| !is_identifier(reference))
    {
        return Err(WorkCommandStoreError::InvalidPolicyReferences);
    }
    if receipt.occurred_at > now {
        return Err(WorkCommandStoreError::InvalidReceiptTime);
    }
    Ok(())
}

pub(super) fn request_digest(request: &NewWorkCommand) -> Result<Vec<u8>, WorkCommandStoreError> {
    let mut hasher = Sha256::new();
    append_bytes(&mut hasher, b"mindleak.ackplane.work-command.request.v1");
    for value in [
        request.tenant_id.as_bytes(),
        request.repository_id.as_bytes(),
        request.command_id.as_bytes(),
        request.schema_version.as_bytes(),
        request.issuing_principal_id.as_bytes(),
        request.rationale.as_bytes(),
        request.idempotency_key.as_bytes(),
    ] {
        append_bytes(&mut hasher, value);
    }
    hasher.update(request.kind.as_i16().to_be_bytes());
    append_optional_bytes(&mut hasher, request.task_id.as_deref());
    append_optional_bytes(&mut hasher, request.delegation_id.as_deref());
    append_optional_bytes(&mut hasher, request.confirmation_id.as_deref());
    append_identifiers(&mut hasher, &request.policy_refs);
    match request.expected_task_version {
        Some(version) => {
            hasher.update([1]);
            hasher.update(version.to_be_bytes());
        }
        None => hasher.update([0]),
    }
    append_timestamp(&mut hasher, request.expires_at)?;
    append_bytes(&mut hasher, &request.payload_digest);
    Ok(hasher.finalize().to_vec())
}

pub(super) fn receipt_digest(
    receipt: &NewWorkCommandReceipt,
) -> Result<Vec<u8>, WorkCommandStoreError> {
    let mut hasher = Sha256::new();
    append_bytes(&mut hasher, b"mindleak.ackplane.work-command.receipt.v1");
    for value in [
        receipt.tenant_id.as_bytes(),
        receipt.repository_id.as_bytes(),
        receipt.command_id.as_bytes(),
        receipt.receipt_id.as_bytes(),
        receipt.reason.as_bytes(),
    ] {
        append_bytes(&mut hasher, value);
    }
    hasher.update(receipt.outcome.as_i16().to_be_bytes());
    append_identifiers(&mut hasher, &receipt.evidence_refs);
    append_timestamp(&mut hasher, receipt.occurred_at)?;
    Ok(hasher.finalize().to_vec())
}

pub(super) fn command_from_row(row: &Row) -> Result<WorkCommand, WorkCommandStoreError> {
    Ok(WorkCommand {
        tenant_id: row.get("tenant_id"),
        repository_id: row.get("repository_id"),
        command_id: row.get("command_id"),
        kind: WorkCommandKind::from_i16(row.get("command_kind"))?,
        schema_version: row.get("schema_version"),
        task_id: row.get("task_id"),
        issuing_principal_id: row.get("issuing_principal_id"),
        delegation_id: row.get("delegation_id"),
        policy_refs: row.get("policy_refs"),
        rationale: row.get("rationale"),
        expected_task_version: row.get("expected_task_version"),
        confirmation_id: row.get("confirmation_id"),
        expires_at: row.get("expires_at"),
        idempotency_key: row.get("idempotency_key"),
        request_digest: row.get("request_digest"),
        payload_digest: row.get("payload_digest"),
        recorded_at: row.get("recorded_at"),
    })
}

pub(super) fn receipt_from_row(row: &Row) -> Result<WorkCommandReceipt, WorkCommandStoreError> {
    Ok(WorkCommandReceipt {
        tenant_id: row.get("tenant_id"),
        repository_id: row.get("repository_id"),
        command_id: row.get("command_id"),
        receipt_id: row.get("receipt_id"),
        outcome: WorkCommandOutcome::from_i16(row.get("outcome"))?,
        reason: row.get("reason"),
        evidence_refs: row.get("evidence_refs"),
        receipt_digest: row.get("receipt_digest"),
        occurred_at: row.get("occurred_at"),
        recorded_at: row.get("recorded_at"),
    })
}

fn require_identifier(field: &'static str, value: &str) -> Result<(), WorkCommandStoreError> {
    if is_identifier(value) {
        Ok(())
    } else {
        Err(WorkCommandStoreError::InvalidIdentifier { field })
    }
}

fn validate_optional_identifier(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), WorkCommandStoreError> {
    if value.is_none_or(is_identifier) {
        Ok(())
    } else {
        Err(WorkCommandStoreError::InvalidOptionalIdentifier { field })
    }
}

fn is_identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_IDENTIFIER_BYTES
}

fn append_bytes(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn append_optional_bytes(hasher: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            hasher.update([1]);
            append_bytes(hasher, value.as_bytes());
        }
        None => hasher.update([0]),
    }
}

fn append_identifiers(hasher: &mut Sha256, values: &[String]) {
    hasher.update((values.len() as u64).to_be_bytes());
    for value in values {
        append_bytes(hasher, value.as_bytes());
    }
}

fn append_timestamp(
    hasher: &mut Sha256,
    timestamp: SystemTime,
) -> Result<(), WorkCommandStoreError> {
    let duration = timestamp
        .duration_since(UNIX_EPOCH)
        .map_err(|_| WorkCommandStoreError::InvalidTimestamp)?;
    hasher.update(duration.as_secs().to_be_bytes());
    hasher.update(duration.subsec_nanos().to_be_bytes());
    Ok(())
}

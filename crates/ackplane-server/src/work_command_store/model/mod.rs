use std::time::SystemTime;

use thiserror::Error;

use crate::directive_store::DirectiveStoreError;

mod digest;
mod validation;

pub(in crate::work_command_store) use digest::{
    append_bytes, append_optional_bytes, append_timestamp, assigned_command_id, command_from_row,
    receipt_digest, receipt_from_row, request_digest,
};
pub(in crate::work_command_store) use validation::{validate_receipt, validate_request};

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

/// An immutable command request after the command service has canonicalized
/// its payload. Ackplane assigns the durable command id from its scoped
/// idempotency identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWorkCommand {
    pub tenant_id: String,
    pub repository_id: String,
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
    /// The ADR-0107 directive this command's confirm step issued, once one
    /// has been (Assign/Steer/Pause/Resume/Drain only). `None` for the five
    /// server-owned kinds, and for a supervisor-directed command that has not
    /// yet been confirmed.
    pub directive_id: Option<String>,
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
    #[error("a supervisor-directed command's payload does not name a closed directive kind")]
    InvalidDirectivePayload,
    #[error("issuing the supervisor-directed command's directive failed: {0}")]
    Directive(#[from] DirectiveStoreError),
}

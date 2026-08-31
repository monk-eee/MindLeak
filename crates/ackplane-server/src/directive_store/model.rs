//! Public records and errors for the directive ledger.

use std::time::SystemTime;

use ackplane_protocol::{
    supervisor::{
        directive_payload_digest, directive_requirement, DirectiveRequirement, SupervisorError,
    },
    v1,
};
use prost::Message;
use sha2::{Digest, Sha256};
use thiserror::Error;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const DIGEST_BYTES: usize = 32;
const MAX_IDENTIFIER_BYTES: usize = 256;
const MAX_RATIONALE_BYTES: usize = 4_096;
const MAX_REFS: usize = 32;
const MAX_PAYLOAD_BYTES: usize = 16_384;
const MAX_DIAGNOSTIC_BYTES: usize = 4_096;

/// One immutable directive as normalized and sequenced by Ackplane.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectiveRecord {
    pub directive: v1::AgentDirective,
    pub request_digest: Vec<u8>,
    pub recorded_at: SystemTime,
}

/// One immutable receipt appended for a directive evaluation or outcome.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectiveReceiptRecord {
    pub receipt_position: u64,
    pub receipt: v1::DirectiveReceipt,
    pub receipt_digest: Vec<u8>,
    pub recorded_at: SystemTime,
}

/// The result of enqueueing a directive, including an idempotent replay.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectiveWriteOutcome {
    pub record: DirectiveRecord,
    pub idempotent_replay: bool,
}

/// The result of recording a directive receipt, including an idempotent retry.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectiveReceiptOutcome {
    pub record: DirectiveReceiptRecord,
    pub idempotent_replay: bool,
}

#[derive(Debug, Error)]
pub enum DirectiveStoreError {
    #[error("directive database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("directive store could not obtain a database connection: {0}")]
    PoolExhausted(#[from] deadpool_postgres::PoolError),
    #[error("stored directive data cannot be decoded: {0}")]
    Decode(#[from] prost::DecodeError),
    #[error("stored supervisor capabilities are invalid: {0}")]
    Capabilities(#[from] serde_json::Error),
    #[error("stored supervisor capabilities are invalid: {0}")]
    Supervisor(#[from] SupervisorError),
    #[error("{field} must be a bounded non-empty identifier")]
    InvalidIdentifier { field: &'static str },
    #[error("{field} must be absent or a bounded non-empty identifier")]
    InvalidOptionalIdentifier { field: &'static str },
    #[error("{field} must be exactly 32 bytes")]
    InvalidDigest { field: &'static str },
    #[error("directive payload digest does not bind its typed payload")]
    PayloadDigestMismatch,
    #[error("directive payload must be a bounded closed protocol message")]
    InvalidPayload,
    #[error("directive creation or expiry timestamp is invalid")]
    InvalidTimestamp,
    #[error("directive expiry must follow creation and remain in the future")]
    InvalidTimeWindow,
    #[error("directive sequence must be allocated by Ackplane")]
    ClientSequenceForbidden,
    #[error("directive creation time must be allocated by Ackplane")]
    ClientCreationTimeForbidden,
    #[error("directive kind and payload do not name a closed capability")]
    UnsupportedDirective,
    #[error("directive required capability does not match its typed payload")]
    RequiredCapabilityMismatch,
    #[error("directive target has no registered supervisor session")]
    UnknownSupervisorSession,
    #[error("directive target node does not match the registered supervisor session")]
    TargetNodeMismatch,
    #[error("target supervisor does not advertise the required directive capability")]
    CapabilityMissing,
    #[error("directive id or idempotency key was replayed with different content")]
    IdempotencyConflict,
    #[error("directive stream position is exhausted")]
    SequenceExhausted,
    #[error("directive receipt does not match its stored directive target or payload")]
    ReceiptMismatch,
    #[error("directive receipt references an unknown directive")]
    UnknownDirective,
    #[error("directive receipt has an unknown status or reason")]
    InvalidReceiptOutcome,
    #[error("stored directive sequence or receipt position is invalid")]
    InvalidStoredPosition,
}

pub(super) fn validate_directive(
    directive: &v1::AgentDirective,
    now: SystemTime,
) -> Result<(DirectiveRequirement, SystemTime), DirectiveStoreError> {
    for (field, value) in [
        ("directive_id", directive.directive_id.as_str()),
        ("tenant_id", directive.tenant_id.as_str()),
        ("repository_id", directive.repository_id.as_str()),
        ("project_id", directive.project_id.as_str()),
        ("target_node_id", directive.target_node_id.as_str()),
        (
            "target_agent_session_id",
            directive.target_agent_session_id.as_str(),
        ),
        ("schema_version", directive.schema_version.as_str()),
        (
            "issuing_principal_id",
            directive.issuing_principal_id.as_str(),
        ),
        ("idempotency_key", directive.idempotency_key.as_str()),
        (
            "required_capability",
            directive.required_capability.as_str(),
        ),
    ] {
        require_identifier(field, value)?;
    }
    validate_text("rationale", &directive.rationale, MAX_RATIONALE_BYTES)?;
    for (field, value) in [
        ("task_id", directive.task_id.as_str()),
        ("goal_id", directive.goal_id.as_str()),
        ("context_packet_id", directive.context_packet_id.as_str()),
    ] {
        validate_optional_identifier(field, value)?;
    }
    for (field, values) in [
        ("policy_refs", directive.policy_refs.as_slice()),
        ("knowledge_refs", directive.knowledge_refs.as_slice()),
        ("evidence_refs", directive.evidence_refs.as_slice()),
    ] {
        validate_references(field, values)?;
    }
    if directive.payload_digest.len() != DIGEST_BYTES {
        return Err(DirectiveStoreError::InvalidDigest {
            field: "payload_digest",
        });
    }
    if directive.sequence != 0 {
        return Err(DirectiveStoreError::ClientSequenceForbidden);
    }
    if !directive.created_at.is_empty() {
        return Err(DirectiveStoreError::ClientCreationTimeForbidden);
    }
    let requirement =
        directive_requirement(directive).ok_or(DirectiveStoreError::UnsupportedDirective)?;
    if directive.payload_digest
        != directive_payload_digest(directive).ok_or(DirectiveStoreError::UnsupportedDirective)?
    {
        return Err(DirectiveStoreError::PayloadDigestMismatch);
    }
    if directive.required_capability != requirement.required_capability {
        return Err(DirectiveStoreError::RequiredCapabilityMismatch);
    }
    if directive.encode_to_vec().len() > MAX_PAYLOAD_BYTES {
        return Err(DirectiveStoreError::InvalidPayload);
    }
    let expires_at = parse_timestamp(&directive.expires_at)?;
    if expires_at <= now {
        return Err(DirectiveStoreError::InvalidTimeWindow);
    }
    Ok((requirement, expires_at))
}

pub(super) fn directive_request_digest(directive: &v1::AgentDirective) -> Vec<u8> {
    Sha256::digest(directive.encode_to_vec()).to_vec()
}

/// The receipt's evidential identity: what the supervisor decided about a
/// directive, deliberately excluding where that decision happened to sit in
/// the supervisor's outbox.
///
/// `outbox_sequence` (ADR-0146) is a transport position, not part of the
/// decision. Including it would break the replay detection this digest exists
/// for: when Ackplane redelivers a directive, the supervisor's inbox replays a
/// byte-identical receipt, but its outbox assigns that resend a *new*
/// sequence. Hashing the sequence in would make one unchanged decision look
/// like two different ones and record a duplicate receipt row against the
/// directive.
///
/// Clearing the field rather than hashing selected fields keeps this digest
/// bit-identical to the one computed before ADR-0146 for every receipt that
/// carries no sequence, so no already-stored digest changes meaning.
pub(super) fn receipt_digest(receipt: &v1::DirectiveReceipt) -> Vec<u8> {
    let mut decision = receipt.clone();
    decision.outbox_sequence = None;
    Sha256::digest(decision.encode_to_vec()).to_vec()
}

pub(super) fn validate_receipt(
    receipt: &v1::DirectiveReceipt,
    directive: &v1::AgentDirective,
    directive_created_at: SystemTime,
) -> Result<SystemTime, DirectiveStoreError> {
    for (field, value) in [
        ("directive_id", receipt.directive_id.as_str()),
        ("tenant_id", receipt.tenant_id.as_str()),
        ("repository_id", receipt.repository_id.as_str()),
        ("project_id", receipt.project_id.as_str()),
        ("node_id", receipt.node_id.as_str()),
        ("agent_session_id", receipt.agent_session_id.as_str()),
    ] {
        require_identifier(field, value)?;
    }
    for (field, values) in [
        ("checkpoint_refs", receipt.checkpoint_refs.as_slice()),
        ("evidence_refs", receipt.evidence_refs.as_slice()),
    ] {
        validate_references(field, values)?;
    }
    if receipt.diagnostic.len() > MAX_DIAGNOSTIC_BYTES
        || receipt.payload_digest.len() != DIGEST_BYTES
    {
        return Err(DirectiveStoreError::InvalidPayload);
    }
    let status = v1::DirectiveReceiptStatus::try_from(receipt.status).ok();
    let reason = v1::DirectiveReceiptReason::try_from(receipt.reason).ok();
    if matches!(status, None | Some(v1::DirectiveReceiptStatus::Unspecified))
        || matches!(reason, None | Some(v1::DirectiveReceiptReason::Unspecified))
    {
        return Err(DirectiveStoreError::InvalidReceiptOutcome);
    }
    if receipt.directive_id != directive.directive_id
        || receipt.tenant_id != directive.tenant_id
        || receipt.repository_id != directive.repository_id
        || receipt.project_id != directive.project_id
        || receipt.node_id != directive.target_node_id
        || receipt.agent_session_id != directive.target_agent_session_id
        || receipt.directive_sequence != directive.sequence
        || receipt.payload_digest != directive.payload_digest
    {
        return Err(DirectiveStoreError::ReceiptMismatch);
    }
    let occurred_at = parse_timestamp(&receipt.occurred_at)?;
    if occurred_at < directive_created_at {
        return Err(DirectiveStoreError::InvalidTimeWindow);
    }
    if receipt.encode_to_vec().len() > MAX_PAYLOAD_BYTES {
        return Err(DirectiveStoreError::InvalidPayload);
    }
    Ok(occurred_at)
}

pub(super) fn directive_from_row(
    row: &tokio_postgres::Row,
) -> Result<DirectiveRecord, DirectiveStoreError> {
    let payload: Vec<u8> = row.get("directive_payload");
    Ok(DirectiveRecord {
        directive: v1::AgentDirective::decode(payload.as_slice())?,
        request_digest: row.get("request_digest"),
        recorded_at: row.get("recorded_at"),
    })
}

pub(super) fn receipt_from_row(
    row: &tokio_postgres::Row,
) -> Result<DirectiveReceiptRecord, DirectiveStoreError> {
    let receipt_position = u64::try_from(row.get::<_, i64>("receipt_position"))
        .map_err(|_| DirectiveStoreError::InvalidStoredPosition)?;
    let payload: Vec<u8> = row.get("receipt_payload");
    Ok(DirectiveReceiptRecord {
        receipt_position,
        receipt: v1::DirectiveReceipt::decode(payload.as_slice())?,
        receipt_digest: row.get("receipt_digest"),
        recorded_at: row.get("recorded_at"),
    })
}

pub(super) fn normalize_timestamp(timestamp: SystemTime) -> SystemTime {
    let timestamp = OffsetDateTime::from(timestamp);
    let remainder = timestamp.nanosecond() % 1_000;
    (timestamp - time::Duration::nanoseconds(i64::from(remainder))).into()
}

pub(super) fn format_timestamp(timestamp: SystemTime) -> Result<String, DirectiveStoreError> {
    OffsetDateTime::from(normalize_timestamp(timestamp))
        .format(&Rfc3339)
        .map_err(|_| DirectiveStoreError::InvalidTimestamp)
}

fn parse_timestamp(value: &str) -> Result<SystemTime, DirectiveStoreError> {
    let timestamp = OffsetDateTime::parse(value, &Rfc3339)
        .map_err(|_| DirectiveStoreError::InvalidTimestamp)?;
    if timestamp.unix_timestamp() < 0 {
        return Err(DirectiveStoreError::InvalidTimestamp);
    }
    Ok(timestamp.into())
}

fn require_identifier(field: &'static str, value: &str) -> Result<(), DirectiveStoreError> {
    if value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES {
        return Err(DirectiveStoreError::InvalidIdentifier { field });
    }
    Ok(())
}

fn validate_optional_identifier(
    field: &'static str,
    value: &str,
) -> Result<(), DirectiveStoreError> {
    if value.is_empty() {
        return Ok(());
    }
    require_identifier(field, value)
        .map_err(|_| DirectiveStoreError::InvalidOptionalIdentifier { field })
}

fn validate_text(
    field: &'static str,
    value: &str,
    max_bytes: usize,
) -> Result<(), DirectiveStoreError> {
    if value.trim().is_empty() || value.len() > max_bytes {
        return Err(DirectiveStoreError::InvalidIdentifier { field });
    }
    Ok(())
}

fn validate_references(field: &'static str, values: &[String]) -> Result<(), DirectiveStoreError> {
    if values.len() > MAX_REFS {
        return Err(DirectiveStoreError::InvalidIdentifier { field });
    }
    for value in values {
        require_identifier(field, value)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt() -> v1::DirectiveReceipt {
        v1::DirectiveReceipt {
            directive_id: "directive:digest".to_string(),
            tenant_id: "tenant:digest".to_string(),
            project_id: "project:digest".to_string(),
            repository_id: "repository:digest".to_string(),
            node_id: "node:digest".to_string(),
            agent_session_id: "session:digest".to_string(),
            status: v1::DirectiveReceiptStatus::Applied as i32,
            reason: v1::DirectiveReceiptReason::None as i32,
            occurred_at: "2026-08-30T00:00:00Z".to_string(),
            payload_digest: vec![7; 32],
            checkpoint_refs: Vec::new(),
            evidence_refs: Vec::new(),
            directive_sequence: 3,
            diagnostic: String::new(),
            outbox_sequence: None,
        }
    }

    /// Regression: one decision resent from a different outbox slot must stay
    /// one receipt, not become two.
    ///
    /// THE BUG THIS PREVENTS. `receipt_digest` is the receipt's idempotency key
    /// (`ON CONFLICT (tenant_id, repository_id, directive_id, receipt_digest)`).
    /// ADR-0146 added `outbox_sequence` to `DirectiveReceipt`, and hashing the
    /// whole encoded message would have folded that sequence into the key. When
    /// Ackplane redelivers a directive, the supervisor's inbox replays a
    /// byte-identical decision but its outbox assigns the resend a *new*
    /// sequence -- so the server would have seen two different digests for one
    /// unchanged decision and written a duplicate receipt row against the
    /// directive, silently inflating the durable record it exists to protect.
    ///
    /// Fix: `receipt_digest` clears `outbox_sequence` before hashing, so the
    /// digest identifies what the supervisor decided, not where that decision
    /// happened to sit in its outbox.
    #[test]
    fn receipt_digest_ignores_the_outbox_slot_a_decision_was_sent_from() {
        let mut first_attempt = receipt();
        first_attempt.outbox_sequence = Some(5);
        let mut resent_from_a_later_slot = receipt();
        resent_from_a_later_slot.outbox_sequence = Some(6);

        assert_eq!(
            receipt_digest(&first_attempt),
            receipt_digest(&resent_from_a_later_slot),
            "the same decision sent from a different outbox slot must keep one identity"
        );
    }

    /// The sequence is excluded rather than merely normalised: a receipt that
    /// carries one must hash exactly as it did before ADR-0146 existed, so no
    /// digest already stored against a directive changes meaning.
    #[test]
    fn receipt_digest_is_unchanged_by_adding_an_outbox_sequence() {
        let without_sequence = receipt();
        let mut with_sequence = receipt();
        with_sequence.outbox_sequence = Some(42);

        assert_eq!(
            receipt_digest(&without_sequence),
            receipt_digest(&with_sequence)
        );
    }

    /// The exclusion is narrow. Everything that describes the decision itself
    /// still separates one receipt from another -- otherwise the test above
    /// would pass just as well against a digest that ignored the whole message.
    #[test]
    fn receipt_digest_still_separates_genuinely_different_decisions() {
        let accepted = receipt();
        let mut refused = receipt();
        refused.status = v1::DirectiveReceiptStatus::Refused as i32;

        assert_ne!(receipt_digest(&accepted), receipt_digest(&refused));
    }
}

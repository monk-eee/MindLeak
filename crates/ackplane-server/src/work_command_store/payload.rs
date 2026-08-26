//! Per-kind Work-command payloads (ADR-0125 decision 4). `work_commands`
//! stores only a digest of this content; the caller re-presents the exact
//! payload when confirming, and the service refuses to execute a payload
//! whose digest no longer matches the one fixed at submission (decision 8:
//! "changing any field ... requires a new preview").

use std::time::SystemTime;

use sha2::{Digest, Sha256};

use super::model::{
    append_bytes, append_optional_bytes, append_timestamp, WorkCommandKind, WorkCommandStoreError,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CreateWorkPayload {
    pub(super) task_id: String,
    pub(super) title: String,
    pub(super) acceptance: String,
    pub(super) goal_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RouteWorkPayload {
    pub(super) route_reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReleaseLeasePayload {
    pub(super) expected_owner_id: String,
    pub(super) expected_lease_expires_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AnswerWaitPayload {
    pub(super) wait_id: String,
    pub(super) answer: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReviewDisposition {
    Accept,
    RequestChanges,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SubmitReviewPayload {
    pub(super) disposition: ReviewDisposition,
    pub(super) rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WorkCommandPayload {
    CreateWork(CreateWorkPayload),
    RouteWork(RouteWorkPayload),
    ReleaseLease(ReleaseLeasePayload),
    AnswerWait(AnswerWaitPayload),
    SubmitReview(SubmitReviewPayload),
}

impl WorkCommandPayload {
    pub(super) fn kind(&self) -> WorkCommandKind {
        match self {
            Self::CreateWork(_) => WorkCommandKind::CreateWork,
            Self::RouteWork(_) => WorkCommandKind::RouteWork,
            Self::ReleaseLease(_) => WorkCommandKind::ReleaseLease,
            Self::AnswerWait(_) => WorkCommandKind::AnswerWait,
            Self::SubmitReview(_) => WorkCommandKind::SubmitReview,
        }
    }
}

/// A payload whose kind disagrees with its command's recorded kind can never
/// be executed, immutable-preview or not: the command ledger's own closed
/// vocabulary is the authority on what a command id may mean.
pub(super) fn payload_matches_kind(payload: &WorkCommandPayload, kind: WorkCommandKind) -> bool {
    payload.kind() == kind
}

pub(super) fn payload_digest(
    payload: &WorkCommandPayload,
) -> Result<Vec<u8>, WorkCommandStoreError> {
    let mut hasher = Sha256::new();
    append_bytes(&mut hasher, b"mindleak.ackplane.work-command.payload.v1");
    hasher.update(payload.kind().as_i16().to_be_bytes());
    match payload {
        WorkCommandPayload::CreateWork(payload) => {
            append_bytes(&mut hasher, payload.task_id.as_bytes());
            append_bytes(&mut hasher, payload.title.as_bytes());
            append_bytes(&mut hasher, payload.acceptance.as_bytes());
            append_optional_bytes(&mut hasher, payload.goal_id.as_deref());
        }
        WorkCommandPayload::RouteWork(payload) => {
            append_bytes(&mut hasher, payload.route_reference.as_bytes());
        }
        WorkCommandPayload::ReleaseLease(payload) => {
            append_bytes(&mut hasher, payload.expected_owner_id.as_bytes());
            append_timestamp(&mut hasher, payload.expected_lease_expires_at)?;
        }
        WorkCommandPayload::AnswerWait(payload) => {
            append_bytes(&mut hasher, payload.wait_id.as_bytes());
            append_bytes(&mut hasher, payload.answer.as_bytes());
        }
        WorkCommandPayload::SubmitReview(payload) => {
            let tag: u8 = match payload.disposition {
                ReviewDisposition::Accept => 1,
                ReviewDisposition::RequestChanges => 0,
            };
            hasher.update([tag]);
            append_bytes(&mut hasher, payload.rationale.as_bytes());
        }
    }
    Ok(hasher.finalize().to_vec())
}

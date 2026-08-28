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
pub struct CreateWorkPayload {
    pub task_id: String,
    pub title: String,
    pub acceptance: String,
    pub goal_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteWorkPayload {
    pub route_reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseLeasePayload {
    pub expected_owner_id: String,
    pub expected_lease_expires_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerWaitPayload {
    pub wait_id: String,
    pub answer: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewDisposition {
    Accept,
    RequestChanges,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitReviewPayload {
    pub disposition: ReviewDisposition,
    pub rationale: String,
}

/// The enrolled supervisor session an ADR-0107 directive addresses. Every
/// supervisor-directed payload names its own target explicitly rather than
/// inferring one from the task's current claim: `Assign` by definition has
/// no prior claim to infer from, and inferring one for the other four would
/// make the same "which session" decision two different ways depending on
/// kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveTarget {
    pub target_node_id: String,
    pub target_session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignPayload {
    pub target: DirectiveTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteerPayload {
    pub target: DirectiveTarget,
    pub instruction: String,
    pub checkpoint_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PausePayload {
    pub target: DirectiveTarget,
    pub checkpoint_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumePayload {
    pub target: DirectiveTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrainPayload {
    pub target: DirectiveTarget,
    pub deadline: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkCommandPayload {
    CreateWork(CreateWorkPayload),
    RouteWork(RouteWorkPayload),
    ReleaseLease(ReleaseLeasePayload),
    AnswerWait(AnswerWaitPayload),
    SubmitReview(SubmitReviewPayload),
    Assign(AssignPayload),
    Steer(SteerPayload),
    Pause(PausePayload),
    Resume(ResumePayload),
    Drain(DrainPayload),
}

impl WorkCommandPayload {
    pub fn kind(&self) -> WorkCommandKind {
        match self {
            Self::CreateWork(_) => WorkCommandKind::CreateWork,
            Self::RouteWork(_) => WorkCommandKind::RouteWork,
            Self::ReleaseLease(_) => WorkCommandKind::ReleaseLease,
            Self::AnswerWait(_) => WorkCommandKind::AnswerWait,
            Self::SubmitReview(_) => WorkCommandKind::SubmitReview,
            Self::Assign(_) => WorkCommandKind::Assign,
            Self::Steer(_) => WorkCommandKind::Steer,
            Self::Pause(_) => WorkCommandKind::Pause,
            Self::Resume(_) => WorkCommandKind::Resume,
            Self::Drain(_) => WorkCommandKind::Drain,
        }
    }
}

/// A payload whose kind disagrees with its command's recorded kind can never
/// be executed, immutable-preview or not: the command ledger's own closed
/// vocabulary is the authority on what a command id may mean.
pub fn payload_matches_kind(payload: &WorkCommandPayload, kind: WorkCommandKind) -> bool {
    payload.kind() == kind
}

pub fn payload_digest(payload: &WorkCommandPayload) -> Result<Vec<u8>, WorkCommandStoreError> {
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
        WorkCommandPayload::Assign(payload) => {
            append_target(&mut hasher, &payload.target);
        }
        WorkCommandPayload::Steer(payload) => {
            append_target(&mut hasher, &payload.target);
            append_bytes(&mut hasher, payload.instruction.as_bytes());
            hasher.update([u8::from(payload.checkpoint_required)]);
        }
        WorkCommandPayload::Pause(payload) => {
            append_target(&mut hasher, &payload.target);
            hasher.update([u8::from(payload.checkpoint_required)]);
        }
        WorkCommandPayload::Resume(payload) => {
            append_target(&mut hasher, &payload.target);
        }
        WorkCommandPayload::Drain(payload) => {
            append_target(&mut hasher, &payload.target);
            append_timestamp(&mut hasher, payload.deadline)?;
        }
    }
    Ok(hasher.finalize().to_vec())
}

fn append_target(hasher: &mut Sha256, target: &DirectiveTarget) {
    append_bytes(hasher, target.target_node_id.as_bytes());
    append_bytes(hasher, target.target_session_id.as_bytes());
}

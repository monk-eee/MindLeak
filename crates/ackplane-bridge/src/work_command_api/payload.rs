//! Wire payload shapes for ADR-0125's Work commands and their conversion to
//! the store's typed [`WorkCommandPayload`]. Split out of `mod.rs` to keep
//! the routing/handler module focused on the HTTP boundary.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ackplane_server::work_command_store::{
    AnswerWaitPayload, AssignPayload, CreateWorkPayload, DirectiveTarget, DrainPayload,
    PausePayload, ReleaseLeasePayload, ResumePayload, ReviewDisposition, RouteWorkPayload,
    SteerPayload, SubmitReviewPayload, WorkCommandPayload,
};
use axum::http::StatusCode;
use serde::Deserialize;

/// The ten ADR-0125 payload shapes as they arrive over JSON. Field names and
/// the `kind` tag match `work_command_vocabulary::WORK_COMMAND_OPERATIONS`
/// exactly (`rename_all = "snake_case"` on these variant names produces the
/// same strings), so a client never has to maintain a second vocabulary.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum WorkCommandPayloadRequest {
    CreateWork {
        task_id: String,
        title: String,
        acceptance: String,
        #[serde(default)]
        goal_id: Option<String>,
    },
    RouteWork {
        route_reference: String,
    },
    ReleaseLease {
        expected_owner_id: String,
        expected_lease_expires_at_seconds: u64,
    },
    AnswerWait {
        wait_id: String,
        answer: String,
    },
    SubmitReview {
        /// `"accept"` or `"request_changes"`.
        disposition: String,
        /// Distinct wire name from the command envelope's own `rationale`
        /// (why the command was issued) -- `#[serde(flatten)]` merges this
        /// enum's fields into the same JSON object as
        /// `SubmitWorkCommandRequest`, so a shared field name would force
        /// both to read the identical value instead of naming two distinct
        /// things.
        review_rationale: String,
    },
    Assign {
        target_node_id: String,
        target_session_id: String,
    },
    Steer {
        target_node_id: String,
        target_session_id: String,
        instruction: String,
        checkpoint_required: bool,
    },
    Pause {
        target_node_id: String,
        target_session_id: String,
        checkpoint_required: bool,
    },
    Resume {
        target_node_id: String,
        target_session_id: String,
    },
    Drain {
        target_node_id: String,
        target_session_id: String,
        deadline_seconds: u64,
    },
}

pub(super) fn unix_seconds_to_system_time(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn directive_target(target_node_id: String, target_session_id: String) -> DirectiveTarget {
    DirectiveTarget {
        target_node_id,
        target_session_id,
    }
}

/// Translates the wire payload into the store's typed payload. The only
/// rejectable shape today is `SubmitReview`'s free-text disposition, which
/// must name one of the two closed dispositions.
pub(super) fn build_payload(
    request: WorkCommandPayloadRequest,
) -> Result<WorkCommandPayload, StatusCode> {
    Ok(match request {
        WorkCommandPayloadRequest::CreateWork {
            task_id,
            title,
            acceptance,
            goal_id,
        } => WorkCommandPayload::CreateWork(CreateWorkPayload {
            task_id,
            title,
            acceptance,
            goal_id,
        }),
        WorkCommandPayloadRequest::RouteWork { route_reference } => {
            WorkCommandPayload::RouteWork(RouteWorkPayload { route_reference })
        }
        WorkCommandPayloadRequest::ReleaseLease {
            expected_owner_id,
            expected_lease_expires_at_seconds,
        } => WorkCommandPayload::ReleaseLease(ReleaseLeasePayload {
            expected_owner_id,
            expected_lease_expires_at: unix_seconds_to_system_time(
                expected_lease_expires_at_seconds,
            ),
        }),
        WorkCommandPayloadRequest::AnswerWait { wait_id, answer } => {
            WorkCommandPayload::AnswerWait(AnswerWaitPayload { wait_id, answer })
        }
        WorkCommandPayloadRequest::SubmitReview {
            disposition,
            review_rationale,
        } => {
            let disposition = match disposition.as_str() {
                "accept" => ReviewDisposition::Accept,
                "request_changes" => ReviewDisposition::RequestChanges,
                _ => return Err(StatusCode::BAD_REQUEST),
            };
            WorkCommandPayload::SubmitReview(SubmitReviewPayload {
                disposition,
                rationale: review_rationale,
            })
        }
        WorkCommandPayloadRequest::Assign {
            target_node_id,
            target_session_id,
        } => WorkCommandPayload::Assign(AssignPayload {
            target: directive_target(target_node_id, target_session_id),
        }),
        WorkCommandPayloadRequest::Steer {
            target_node_id,
            target_session_id,
            instruction,
            checkpoint_required,
        } => WorkCommandPayload::Steer(SteerPayload {
            target: directive_target(target_node_id, target_session_id),
            instruction,
            checkpoint_required,
        }),
        WorkCommandPayloadRequest::Pause {
            target_node_id,
            target_session_id,
            checkpoint_required,
        } => WorkCommandPayload::Pause(PausePayload {
            target: directive_target(target_node_id, target_session_id),
            checkpoint_required,
        }),
        WorkCommandPayloadRequest::Resume {
            target_node_id,
            target_session_id,
        } => WorkCommandPayload::Resume(ResumePayload {
            target: directive_target(target_node_id, target_session_id),
        }),
        WorkCommandPayloadRequest::Drain {
            target_node_id,
            target_session_id,
            deadline_seconds,
        } => WorkCommandPayload::Drain(DrainPayload {
            target: directive_target(target_node_id, target_session_id),
            deadline: unix_seconds_to_system_time(deadline_seconds),
        }),
    })
}

#[derive(Deserialize)]
pub(super) struct SubmitWorkCommandRequest {
    pub(super) issuing_principal_id: String,
    pub(super) idempotency_key: String,
    pub(super) rationale: String,
    #[serde(default)]
    pub(super) policy_refs: Vec<String>,
    #[serde(default)]
    pub(super) delegation_id: Option<String>,
    /// Absent for `CreateWork` (there is no prior task to version); required
    /// for the other nine kinds (ADR-0125 decision 5). Named distinctly from
    /// `CreateWork`'s own `task_id` payload field (the new task's chosen id)
    /// -- `#[serde(flatten)]` cannot reliably serve two same-named fields
    /// from one shared JSON key, so this envelope never uses that name.
    #[serde(default)]
    pub(super) existing_task_id: Option<String>,
    #[serde(default)]
    pub(super) expected_task_version: Option<i64>,
    pub(super) expires_at_seconds: u64,
    #[serde(flatten)]
    pub(super) payload: WorkCommandPayloadRequest,
}

#[derive(Deserialize)]
pub(super) struct ConfirmWorkCommandRequest {
    #[serde(flatten)]
    pub(super) payload: WorkCommandPayloadRequest,
}

//! ADR-0125's Work command request-and-receipt routes: the Bridge's control
//! surface for `CreateWork`/`RouteWork`/`ReleaseLease`/`AnswerWait`/
//! `SubmitReview` and the ADR-0107 supervisor-directed `Assign`/`Steer`/
//! `Pause`/`Resume`/`Drain`.
//!
//! The Bridge's loopback developer profile is not a verified principal
//! (ADR-0125 decision 2): it derives a single tenant token, not an
//! accountable operator identity. So every request here resolves to
//! [`WorkCommandAuthorization::LoopbackDevelopment`] and every command
//! surfaces a typed `authorization_unavailable` outcome rather than
//! executing. That is not a placeholder to remove later by finding a
//! shortcut -- it is decision 2's safety boundary, enforced the same way for
//! every one of the ten commands. What this module proves today is the full
//! request/response contract end to end: a real route, a real
//! [`WorkCommandService`] call, a real typed refusal -- never a route that
//! reaches `WorkStore` or `ClaimStore` directly (the contract violation
//! decision 11 names explicitly).

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ackplane_server::{
    fleet::FleetStore,
    work_command_store::{
        payload_digest, AnswerWaitPayload, AssignPayload, CreateWorkPayload, DirectiveTarget,
        DrainPayload, NewWorkCommand, PausePayload, ReleaseLeasePayload, ResumePayload,
        ReviewDisposition, RouteWorkPayload, SteerPayload, SubmitReviewPayload,
        WorkCommandAuthorization, WorkCommandKind, WorkCommandOutcome, WorkCommandPayload,
        WorkCommandRefusal, WorkCommandService, WorkCommandServiceError, WorkCommandServiceOutcome,
    },
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

/// Dependencies the Bridge entry point injects when it merges this
/// sub-router into the application.
#[derive(Clone)]
pub struct WorkCommandApiState {
    commands: Arc<Mutex<WorkCommandService>>,
    fleet: Arc<FleetStore>,
    tenant_id: Arc<str>,
}

impl WorkCommandApiState {
    pub fn new(
        commands: Arc<Mutex<WorkCommandService>>,
        fleet: Arc<FleetStore>,
        tenant_id: Arc<str>,
    ) -> Self {
        Self {
            commands,
            fleet,
            tenant_id,
        }
    }
}

/// Builds the isolated Work command sub-router.
pub fn work_command_routes(state: WorkCommandApiState) -> Router {
    Router::new()
        .route(
            "/api/v1/repositories/:repository_id/work/commands",
            post(submit_work_command),
        )
        .route(
            "/api/v1/repositories/:repository_id/work/commands/:command_id/confirm",
            post(confirm_work_command),
        )
        .with_state(state)
}

async fn ensure_repository_visible(
    state: &WorkCommandApiState,
    repository_id: &str,
) -> Result<(), StatusCode> {
    match state
        .fleet
        .repository(state.tenant_id.as_ref(), repository_id)
        .await
    {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(error) => {
            tracing::error!(%error, "Bridge Work command repository visibility query failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// The ten ADR-0125 payload shapes as they arrive over JSON. Field names and
/// the `kind` tag match `work_command_vocabulary::WORK_COMMAND_OPERATIONS`
/// exactly (`rename_all = "snake_case"` on these variant names produces the
/// same strings), so a client never has to maintain a second vocabulary.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WorkCommandPayloadRequest {
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

fn unix_seconds_to_system_time(seconds: u64) -> SystemTime {
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
fn build_payload(request: WorkCommandPayloadRequest) -> Result<WorkCommandPayload, StatusCode> {
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
struct SubmitWorkCommandRequest {
    issuing_principal_id: String,
    idempotency_key: String,
    rationale: String,
    #[serde(default)]
    policy_refs: Vec<String>,
    #[serde(default)]
    delegation_id: Option<String>,
    /// Absent for `CreateWork` (there is no prior task to version); required
    /// for the other nine kinds (ADR-0125 decision 5). Named distinctly from
    /// `CreateWork`'s own `task_id` payload field (the new task's chosen id)
    /// -- `#[serde(flatten)]` cannot reliably serve two same-named fields
    /// from one shared JSON key, so this envelope never uses that name.
    #[serde(default)]
    existing_task_id: Option<String>,
    #[serde(default)]
    expected_task_version: Option<i64>,
    expires_at_seconds: u64,
    #[serde(flatten)]
    payload: WorkCommandPayloadRequest,
}

#[derive(Deserialize)]
struct ConfirmWorkCommandRequest {
    #[serde(flatten)]
    payload: WorkCommandPayloadRequest,
}

fn refusal_label(reason: WorkCommandRefusal) -> &'static str {
    match reason {
        WorkCommandRefusal::MissingPrincipal => "missing_principal",
        WorkCommandRefusal::ForgedPrincipal => "forged_principal",
        WorkCommandRefusal::TenantOutOfScope => "tenant_out_of_scope",
        WorkCommandRefusal::RepositoryOutOfScope => "repository_out_of_scope",
        WorkCommandRefusal::CommandNotPermitted => "command_not_permitted",
        WorkCommandRefusal::PolicyNotPermitted => "policy_not_permitted",
        WorkCommandRefusal::DelegationNotPermitted => "delegation_not_permitted",
    }
}

fn outcome_label(outcome: WorkCommandOutcome) -> &'static str {
    match outcome {
        WorkCommandOutcome::PendingConfirmation => "pending_confirmation",
        WorkCommandOutcome::PendingDelivery => "pending_delivery",
        WorkCommandOutcome::Accepted => "accepted",
        WorkCommandOutcome::Applied => "applied",
        WorkCommandOutcome::Failed => "failed",
        WorkCommandOutcome::Expired => "expired",
        WorkCommandOutcome::Conflicted => "conflicted",
        WorkCommandOutcome::Refused => "refused",
    }
}

/// The full outcome vocabulary a caller may see (ADR-0125 decision 9): every
/// case is a first-class, named result -- never an HTTP success standing in
/// for "the worker did it" (decision 7).
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum WorkCommandResponse {
    AuthorizationUnavailable {
        reason: &'static str,
    },
    Refused {
        reason: &'static str,
    },
    PendingConfirmation {
        command_id: String,
        receipt_id: String,
        outcome: &'static str,
        idempotent_replay: bool,
    },
    Executed {
        command_id: String,
        receipt_id: String,
        outcome: &'static str,
        reason: String,
        idempotent_replay: bool,
    },
    CommandNotFound,
}

impl From<WorkCommandServiceOutcome> for WorkCommandResponse {
    fn from(outcome: WorkCommandServiceOutcome) -> Self {
        match outcome {
            WorkCommandServiceOutcome::AuthorizationUnavailable { reason } => {
                Self::AuthorizationUnavailable { reason }
            }
            WorkCommandServiceOutcome::Refused { reason } => Self::Refused {
                reason: refusal_label(reason),
            },
            WorkCommandServiceOutcome::PendingConfirmation {
                command,
                receipt,
                idempotent_replay,
            } => Self::PendingConfirmation {
                command_id: command.command_id,
                receipt_id: receipt.receipt_id,
                outcome: outcome_label(receipt.outcome),
                idempotent_replay,
            },
            WorkCommandServiceOutcome::Executed {
                command,
                receipt,
                idempotent_replay,
            } => Self::Executed {
                command_id: command.command_id,
                receipt_id: receipt.receipt_id,
                outcome: outcome_label(receipt.outcome),
                reason: receipt.reason,
                idempotent_replay,
            },
            WorkCommandServiceOutcome::CommandNotFound => Self::CommandNotFound,
        }
    }
}

fn service_error_status(error: WorkCommandServiceError) -> StatusCode {
    tracing::error!(%error, "Bridge Work command service call failed");
    StatusCode::INTERNAL_SERVER_ERROR
}

async fn submit_work_command(
    State(state): State<WorkCommandApiState>,
    Path(repository_id): Path<String>,
    Json(request): Json<SubmitWorkCommandRequest>,
) -> Result<Json<WorkCommandResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let SubmitWorkCommandRequest {
        issuing_principal_id,
        idempotency_key,
        rationale,
        policy_refs,
        delegation_id,
        existing_task_id,
        expected_task_version,
        expires_at_seconds,
        payload,
    } = request;
    let payload = build_payload(payload)?;
    let kind = payload.kind();
    // The store requires `task_id: None` on the command itself for
    // `CreateWork` ("CreateWork must not name an existing task or expected
    // task version") -- the new task's chosen id lives only in the payload.
    // Every other kind targets an existing task, so it is required here.
    let task_id = if kind == WorkCommandKind::CreateWork {
        None
    } else {
        Some(existing_task_id.ok_or(StatusCode::BAD_REQUEST)?)
    };
    let payload_digest = payload_digest(&payload).map_err(|error| {
        tracing::error!(%error, "Bridge Work command payload digest failed");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let new_command = NewWorkCommand {
        tenant_id: state.tenant_id.to_string(),
        repository_id,
        kind,
        schema_version: "v1".to_owned(),
        task_id,
        issuing_principal_id,
        delegation_id,
        policy_refs,
        rationale,
        expected_task_version,
        // Not the command's own id -- a future confirmation-chaining kind's
        // reference to an earlier command. None for every kind today.
        confirmation_id: None,
        expires_at: unix_seconds_to_system_time(expires_at_seconds),
        idempotency_key,
        payload_digest,
    };
    let mut commands = state.commands.lock().await;
    let outcome = commands
        .submit(
            WorkCommandAuthorization::LoopbackDevelopment,
            new_command,
            SystemTime::now(),
        )
        .await
        .map_err(service_error_status)?;
    Ok(Json(outcome.into()))
}

async fn confirm_work_command(
    State(state): State<WorkCommandApiState>,
    Path((repository_id, command_id)): Path<(String, String)>,
    Json(request): Json<ConfirmWorkCommandRequest>,
) -> Result<Json<WorkCommandResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let payload = build_payload(request.payload)?;
    let mut commands = state.commands.lock().await;
    let outcome = commands
        .confirm(
            WorkCommandAuthorization::LoopbackDevelopment,
            state.tenant_id.as_ref(),
            &repository_id,
            &command_id,
            payload,
            SystemTime::now(),
        )
        .await
        .map_err(service_error_status)?;
    Ok(Json(outcome.into()))
}

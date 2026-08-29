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
//! `WorkCommandService` call, a real typed refusal -- never a route that
//! reaches `WorkStore` or `ClaimStore` directly (the contract violation
//! decision 11 names explicitly).
//!
//! Split across three files to keep each focused: `payload` owns the wire
//! payload shapes and their conversion to the store's typed payload;
//! `response` owns the typed outcome vocabulary; this file owns the HTTP
//! boundary (state, routing, and the two handlers).

mod payload;
mod response;

use std::{sync::Arc, time::SystemTime};

use ackplane_server::{
    fleet::FleetStore,
    work_command_store::{
        payload_digest, NewWorkCommand, WorkCommandAuthorization, WorkCommandKind,
        WorkCommandService, WorkCommandServiceError,
    },
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use tokio::sync::Mutex;

use payload::{
    build_payload, unix_seconds_to_system_time, ConfirmWorkCommandRequest, SubmitWorkCommandRequest,
};
use response::WorkCommandResponse;

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

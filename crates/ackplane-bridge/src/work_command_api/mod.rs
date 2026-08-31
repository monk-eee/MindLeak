//! ADR-0125's Work command request-and-receipt routes: the Bridge's control
//! surface for `CreateWork`/`RouteWork`/`ReleaseLease`/`AnswerWait`/
//! `SubmitReview` and the ADR-0107 supervisor-directed `Assign`/`Steer`/
//! `Pause`/`Resume`/`Drain`.
//!
//! ADR-0128 recognizes the Bridge's hardened loopback developer profile
//! (`state.tenant_id`, the salted `development_tenant_token`) as a real
//! verified principal for a self-hosted, single-tenant deployment -- not a
//! synonym for "no principal." ADR-0142 extends that same recognition to
//! Work commands: every request here now resolves to
//! [`WorkCommandAuthorization::Verified`] ([`verified_principal`]), scoped to
//! exactly the repository already confirmed visible, the full closed
//! command vocabulary, and no adopted policy or delegation (ADR-0142
//! clause 5). Confirmation stays exactly as ADR-0125 decision 8 specified:
//! this changes *who* is asking, never the preview/confirm/digest machinery
//! around *how* a consequential command executes. A non-loopback,
//! multi-tenant deployment is unchanged and unaffected -- ADR-0094's refusal
//! of a non-loopback bind without a production verifier remains the single
//! enforcement point that keeps this profile from reaching one.
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
        payload_digest, NewWorkCommand, VerifiedWorkCommandPrincipal, WorkCommandAuthorization,
        WorkCommandKind, WorkCommandService, WorkCommandServiceError,
    },
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};

use payload::{
    build_payload, unix_seconds_to_system_time, ConfirmWorkCommandRequest, SubmitWorkCommandRequest,
};
use response::WorkCommandResponse;

/// Dependencies the Bridge entry point injects when it merges this
/// sub-router into the application.
#[derive(Clone)]
pub struct WorkCommandApiState {
    commands: Arc<WorkCommandService>,
    fleet: Arc<FleetStore>,
    tenant_id: Arc<str>,
}

impl WorkCommandApiState {
    pub fn new(
        commands: Arc<WorkCommandService>,
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

/// The verified principal ADR-0142 grants the Bridge's hardened loopback
/// profile for a self-hosted, single-tenant deployment (clause 2): the
/// salted `development_tenant_token` itself is both `principal_id` and
/// `tenant_id` (the same value Administration and Constitution proposals
/// already record as the accountable identity), scoped to exactly the
/// repository `ensure_repository_visible` already confirmed reachable, with
/// the full closed command vocabulary and no adopted policy or delegation
/// (clause 5: Work commands do not gain an `AdministrationPolicy`-style
/// policy layer, and a Bridge-originated request is a direct verified human
/// request, never a delegation).
///
/// `pub(crate)` so the read surface can report what this grants rather than
/// keeping a second, hand-written answer beside it. The Work page previously
/// listed every command as `authorization_unavailable` while these routes
/// executed them — two descriptions of one authority, and the more alarming
/// one was the wrong one. Deriving the list from this function means the two
/// cannot drift again: a change to what the principal allows changes what the
/// page reports, in the same edit.
pub(crate) fn verified_principal(tenant_id: &str, repository_id: &str) -> WorkCommandAuthorization {
    WorkCommandAuthorization::Verified(VerifiedWorkCommandPrincipal {
        principal_id: tenant_id.to_string(),
        tenant_id: tenant_id.to_string(),
        repository_ids: vec![repository_id.to_owned()],
        allowed_commands: WorkCommandKind::ALL.to_vec(),
        policy_refs: Vec::new(),
        delegation_id: None,
    })
}

async fn submit_work_command(
    State(state): State<WorkCommandApiState>,
    Path(repository_id): Path<String>,
    Json(request): Json<SubmitWorkCommandRequest>,
) -> Result<Json<WorkCommandResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let authorization = verified_principal(state.tenant_id.as_ref(), &repository_id);
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
    let outcome = state
        .commands
        .submit(authorization, new_command, SystemTime::now())
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
    let authorization = verified_principal(state.tenant_id.as_ref(), &repository_id);
    let payload = build_payload(request.payload)?;
    let outcome = state
        .commands
        .confirm(
            authorization,
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

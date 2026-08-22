//! Browser-safe read resources for durable ADR-0116 supervisor runtime facts.

use std::{sync::Arc, time::SystemTime};

use ackplane_server::{
    fleet::FleetStore,
    supervisor_store::{
        SupervisorDirectiveCapability, SupervisorFreshness, SupervisorLifecycleReason,
        SupervisorOutboxDurability, SupervisorRuntime, SupervisorStore, SupervisorStoreError,
        SupervisorWorkerState,
    },
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::Serialize;

#[derive(Clone)]
pub struct SupervisorApiState {
    supervisors: Arc<SupervisorStore>,
    fleet: Arc<FleetStore>,
    tenant_id: Arc<str>,
}

impl SupervisorApiState {
    pub fn new(
        supervisors: Arc<SupervisorStore>,
        fleet: Arc<FleetStore>,
        tenant_id: Arc<str>,
    ) -> Self {
        Self {
            supervisors,
            fleet,
            tenant_id,
        }
    }
}

pub fn supervisor_routes(state: SupervisorApiState) -> Router {
    Router::new()
        .route(
            "/api/v1/repositories/:repository_id/supervisors",
            get(supervisor_inventory),
        )
        .route(
            "/api/v1/repositories/:repository_id/supervisors/:supervisor_id/sessions",
            get(supervisor_sessions),
        )
        .route(
            "/api/v1/repositories/:repository_id/supervisors/:supervisor_id/sessions/:session_id/lifecycle",
            get(session_lifecycle),
        )
        .with_state(state)
}

#[derive(Serialize)]
struct SupervisorInventoryResponse {
    entries: Vec<SupervisorResponse>,
}

#[derive(Serialize)]
struct SupervisorResponse {
    supervisor_id: String,
    node_id: String,
    supervisor_version: String,
    protocol_version: String,
    supported_directives: Vec<&'static str>,
    supports_checkpoint: bool,
    supports_force_termination: bool,
    outbox_durability: &'static str,
    recoverable_outbox: bool,
    registered_at_seconds: Option<u64>,
    last_heartbeat_at_seconds: Option<i64>,
    freshness: &'static str,
}

#[derive(Serialize)]
struct SupervisorSessionsResponse {
    supervisor_id: String,
    entries: Vec<SupervisorSessionResponse>,
}

#[derive(Serialize)]
struct SupervisorSessionResponse {
    session_id: String,
    worker_id: String,
    runtime: &'static str,
    state: &'static str,
    current_reason: Option<&'static str>,
    started_at_seconds: i64,
    current_occurred_at_seconds: i64,
    recorded_at_seconds: Option<u64>,
}

#[derive(Serialize)]
struct SupervisorLifecycleResponse {
    supervisor_id: String,
    session_id: String,
    entries: Vec<SupervisorLifecycleReceiptResponse>,
}

#[derive(Serialize)]
struct SupervisorLifecycleReceiptResponse {
    receipt_position: i64,
    worker_id: String,
    occurred_at_seconds: i64,
    state: &'static str,
    reason: Option<&'static str>,
    recorded_at_seconds: Option<u64>,
}

async fn supervisor_inventory(
    State(state): State<SupervisorApiState>,
    Path(repository_id): Path<String>,
) -> Result<Json<SupervisorInventoryResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let entries = state
        .supervisors
        .list_supervisors(state.tenant_id.as_ref(), &repository_id)
        .await
        .map_err(supervisor_store_error)?
        .into_iter()
        .map(SupervisorResponse::from)
        .collect();
    Ok(Json(SupervisorInventoryResponse { entries }))
}

async fn supervisor_sessions(
    State(state): State<SupervisorApiState>,
    Path((repository_id, supervisor_id)): Path<(String, String)>,
) -> Result<Json<SupervisorSessionsResponse>, StatusCode> {
    ensure_supervisor_visible(&state, &repository_id, &supervisor_id).await?;
    let entries = state
        .supervisors
        .list_sessions(state.tenant_id.as_ref(), &repository_id, &supervisor_id)
        .await
        .map_err(supervisor_store_error)?
        .into_iter()
        .map(SupervisorSessionResponse::from)
        .collect();
    Ok(Json(SupervisorSessionsResponse {
        supervisor_id,
        entries,
    }))
}

async fn session_lifecycle(
    State(state): State<SupervisorApiState>,
    Path((repository_id, supervisor_id, session_id)): Path<(String, String, String)>,
) -> Result<Json<SupervisorLifecycleResponse>, StatusCode> {
    ensure_supervisor_visible(&state, &repository_id, &supervisor_id).await?;
    let session = state
        .supervisors
        .list_sessions(state.tenant_id.as_ref(), &repository_id, &supervisor_id)
        .await
        .map_err(supervisor_store_error)?
        .into_iter()
        .find(|entry| entry.session.session_id == session_id)
        .ok_or(StatusCode::NOT_FOUND)?;
    let entries = state
        .supervisors
        .lifecycle_history(
            state.tenant_id.as_ref(),
            &repository_id,
            &session.session.session_id,
        )
        .await
        .map_err(supervisor_store_error)?
        .into_iter()
        .map(SupervisorLifecycleReceiptResponse::from)
        .collect();
    Ok(Json(SupervisorLifecycleResponse {
        supervisor_id,
        session_id,
        entries,
    }))
}

impl From<ackplane_server::supervisor_store::SupervisorStatus> for SupervisorResponse {
    fn from(status: ackplane_server::supervisor_store::SupervisorStatus) -> Self {
        let registration = status.registration;
        Self {
            supervisor_id: registration.supervisor_id,
            node_id: registration.identity.node_id,
            supervisor_version: registration.supervisor_version,
            protocol_version: registration.protocol_version,
            supported_directives: registration
                .capabilities
                .supported_directives
                .into_iter()
                .map(directive_label)
                .collect(),
            supports_checkpoint: registration.capabilities.supports_checkpoint,
            supports_force_termination: registration.capabilities.supports_force_termination,
            outbox_durability: outbox_durability_label(registration.capabilities.outbox_durability),
            recoverable_outbox: registration.capabilities.recoverable_outbox,
            registered_at_seconds: unix_seconds(status.registered_at),
            last_heartbeat_at_seconds: status.last_heartbeat_at,
            freshness: freshness_label(status.freshness),
        }
    }
}

impl From<ackplane_server::supervisor_store::SupervisorSessionProjection>
    for SupervisorSessionResponse
{
    fn from(projection: ackplane_server::supervisor_store::SupervisorSessionProjection) -> Self {
        Self {
            session_id: projection.session.session_id,
            worker_id: projection.session.worker_id,
            runtime: runtime_label(projection.session.runtime),
            state: worker_state_label(projection.session.state),
            current_reason: projection.current_reason.map(lifecycle_reason_label),
            started_at_seconds: projection.session.started_at,
            current_occurred_at_seconds: projection.current_occurred_at,
            recorded_at_seconds: unix_seconds(projection.recorded_at),
        }
    }
}

impl From<ackplane_server::supervisor_store::SupervisorLifecycleReceiptRecord>
    for SupervisorLifecycleReceiptResponse
{
    fn from(record: ackplane_server::supervisor_store::SupervisorLifecycleReceiptRecord) -> Self {
        Self {
            receipt_position: record.receipt_position,
            worker_id: record.receipt.worker_id,
            occurred_at_seconds: record.receipt.occurred_at,
            state: worker_state_label(record.receipt.state),
            reason: record.receipt.reason.map(lifecycle_reason_label),
            recorded_at_seconds: unix_seconds(record.recorded_at),
        }
    }
}

async fn ensure_repository_visible(
    state: &SupervisorApiState,
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
            tracing::error!(%error, "Bridge supervisor repository visibility query failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn ensure_supervisor_visible(
    state: &SupervisorApiState,
    repository_id: &str,
    supervisor_id: &str,
) -> Result<(), StatusCode> {
    ensure_repository_visible(state, repository_id).await?;
    let known = state
        .supervisors
        .list_supervisors(state.tenant_id.as_ref(), repository_id)
        .await
        .map_err(supervisor_store_error)?
        .into_iter()
        .any(|entry| entry.registration.supervisor_id == supervisor_id);
    known.then_some(()).ok_or(StatusCode::NOT_FOUND)
}

fn supervisor_store_error(error: SupervisorStoreError) -> StatusCode {
    tracing::error!(%error, "Bridge supervisor query failed");
    StatusCode::INTERNAL_SERVER_ERROR
}

fn directive_label(capability: SupervisorDirectiveCapability) -> &'static str {
    match capability {
        SupervisorDirectiveCapability::Notify => "notify",
        SupervisorDirectiveCapability::Prompt => "prompt",
        SupervisorDirectiveCapability::Assign => "assign",
        SupervisorDirectiveCapability::Steer => "steer",
        SupervisorDirectiveCapability::Pause => "pause",
        SupervisorDirectiveCapability::Resume => "resume",
        SupervisorDirectiveCapability::Drain => "drain",
        SupervisorDirectiveCapability::TerminateGracefully => "terminate_gracefully",
        SupervisorDirectiveCapability::TerminateForce => "terminate_force",
    }
}

fn outbox_durability_label(durability: SupervisorOutboxDurability) -> &'static str {
    match durability {
        SupervisorOutboxDurability::Persistent => "persistent",
        SupervisorOutboxDurability::Ephemeral => "ephemeral",
    }
}

fn freshness_label(freshness: SupervisorFreshness) -> &'static str {
    match freshness {
        SupervisorFreshness::NeverReported => "never_reported",
        SupervisorFreshness::Current => "current",
        SupervisorFreshness::Stale => "stale",
    }
}

fn runtime_label(runtime: SupervisorRuntime) -> &'static str {
    match runtime {
        SupervisorRuntime::LocalMachine => "local_machine",
        SupervisorRuntime::CloudWorker => "cloud_worker",
        SupervisorRuntime::Pipeline => "pipeline",
        SupervisorRuntime::Service => "service",
    }
}

fn worker_state_label(state: SupervisorWorkerState) -> &'static str {
    match state {
        SupervisorWorkerState::Started => "started",
        SupervisorWorkerState::Checkpointed => "checkpointed",
        SupervisorWorkerState::Paused => "paused",
        SupervisorWorkerState::Draining => "draining",
        SupervisorWorkerState::Terminated => "terminated",
        SupervisorWorkerState::Failed => "failed",
        SupervisorWorkerState::Disconnected => "disconnected",
        SupervisorWorkerState::Reconnected => "reconnected",
        SupervisorWorkerState::Completed => "completed",
    }
}

fn lifecycle_reason_label(reason: SupervisorLifecycleReason) -> &'static str {
    match reason {
        SupervisorLifecycleReason::CapabilityMissing => "capability_missing",
        SupervisorLifecycleReason::CheckpointFailed => "checkpoint_failed",
        SupervisorLifecycleReason::DirectiveExpired => "directive_expired",
        SupervisorLifecycleReason::OutboxUnavailable => "outbox_unavailable",
        SupervisorLifecycleReason::ProtocolUnsupported => "protocol_unsupported",
        SupervisorLifecycleReason::SupervisorFailed => "supervisor_failed",
        SupervisorLifecycleReason::WorkerLost => "worker_lost",
    }
}

fn unix_seconds(timestamp: SystemTime) -> Option<u64> {
    timestamp
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

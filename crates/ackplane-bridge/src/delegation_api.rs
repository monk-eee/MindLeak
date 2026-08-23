//! Browser-safe read resources for durable ADR-0115 authority projections.

use std::{sync::Arc, time::SystemTime};

use ackplane_server::{
    delegation_store::{
        DelegatedAction, DelegationEvent, DelegationEventKind, DelegationEventPayload,
        DelegationProjection, DelegationProjectionStatus, DelegationStore, DelegationStoreError,
    },
    fleet::FleetStore,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Html,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

const DELEGATIONS_PAGE: &str = include_str!("../static/delegations.html");
const DEFAULT_LIMIT: i64 = 30;
const MAX_LIMIT: i64 = 100;

#[derive(Clone)]
pub struct DelegationApiState {
    delegations: Arc<DelegationStore>,
    fleet: Arc<FleetStore>,
    tenant_id: Arc<str>,
}

impl DelegationApiState {
    pub fn new(
        delegations: Arc<DelegationStore>,
        fleet: Arc<FleetStore>,
        tenant_id: Arc<str>,
    ) -> Self {
        Self {
            delegations,
            fleet,
            tenant_id,
        }
    }
}

pub fn delegation_routes(state: DelegationApiState) -> Router {
    Router::new()
        .route("/delegations", get(delegations_page))
        .route(
            "/api/v1/repositories/:repository_id/delegations",
            get(delegations),
        )
        .route(
            "/api/v1/repositories/:repository_id/delegations/:delegation_id/history",
            get(delegation_history),
        )
        .with_state(state)
}

#[derive(Deserialize)]
struct DelegationQuery {
    limit: Option<i64>,
}

#[derive(Serialize)]
struct DelegationListResponse {
    entries: Vec<DelegationResponse>,
    effective_limit: i64,
}

#[derive(Serialize)]
struct DelegationResponse {
    delegation_id: String,
    issuer_principal_id: String,
    delegatee_session_id: String,
    project_id: Option<String>,
    task_id: Option<String>,
    goal_id: String,
    policy_version: String,
    constitution_version: String,
    allowed_actions: Vec<&'static str>,
    max_token_budget: u32,
    max_actions_per_session: u32,
    issued_at_seconds: Option<u64>,
    effective_at_seconds: Option<u64>,
    expires_at_seconds: Option<u64>,
    state: &'static str,
    version: u32,
    source_event_position: u64,
    revoked_at_seconds: Option<u64>,
    revoked_by_principal_id: Option<String>,
    revocation_reason: Option<String>,
}

#[derive(Serialize)]
struct DelegationHistoryResponse {
    delegation_id: String,
    entries: Vec<DelegationEventResponse>,
}

#[derive(Serialize)]
struct DelegationEventResponse {
    stream_position: u64,
    kind: &'static str,
    actor_principal_id: String,
    resulting_version: u32,
    recorded_at_seconds: Option<u64>,
    reason: Option<String>,
}

async fn delegations_page() -> Html<&'static str> {
    Html(DELEGATIONS_PAGE)
}

async fn delegations(
    State(state): State<DelegationApiState>,
    Path(repository_id): Path<String>,
    Query(query): Query<DelegationQuery>,
) -> Result<Json<DelegationListResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let effective_limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let entries = state
        .delegations
        .list(state.tenant_id.as_ref(), &repository_id, effective_limit)
        .await
        .map_err(delegation_store_error)?
        .into_iter()
        .map(|projection| DelegationResponse::from_projection(projection, SystemTime::now()))
        .collect();
    Ok(Json(DelegationListResponse {
        entries,
        effective_limit,
    }))
}

async fn delegation_history(
    State(state): State<DelegationApiState>,
    Path((repository_id, delegation_id)): Path<(String, String)>,
) -> Result<Json<DelegationHistoryResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    state
        .delegations
        .get(state.tenant_id.as_ref(), &repository_id, &delegation_id)
        .await
        .map_err(delegation_store_error)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let entries = state
        .delegations
        .history(state.tenant_id.as_ref(), &repository_id, &delegation_id)
        .await
        .map_err(delegation_store_error)?
        .into_iter()
        .map(DelegationEventResponse::from)
        .collect();
    Ok(Json(DelegationHistoryResponse {
        delegation_id,
        entries,
    }))
}

impl DelegationResponse {
    fn from_projection(projection: DelegationProjection, now: SystemTime) -> Self {
        let state = projection_state(&projection, now);
        Self {
            delegation_id: projection.delegation_id,
            issuer_principal_id: projection.issuer_principal_id,
            delegatee_session_id: projection.delegatee_session_id,
            project_id: projection.project_id,
            task_id: projection.task_id,
            goal_id: projection.goal_id,
            policy_version: projection.policy_version,
            constitution_version: projection.constitution_version,
            allowed_actions: projection
                .allowed_actions
                .into_iter()
                .map(action_label)
                .collect(),
            max_token_budget: projection.max_token_budget,
            max_actions_per_session: projection.max_actions_per_session,
            issued_at_seconds: unix_seconds(projection.issued_at),
            effective_at_seconds: unix_seconds(projection.effective_at),
            expires_at_seconds: unix_seconds(projection.expires_at),
            state,
            version: projection.version,
            source_event_position: projection.source_event_position,
            revoked_at_seconds: projection.revoked_at.and_then(unix_seconds),
            revoked_by_principal_id: projection.revoked_by_principal_id,
            revocation_reason: projection.revocation_reason,
        }
    }
}

impl From<DelegationEvent> for DelegationEventResponse {
    fn from(event: DelegationEvent) -> Self {
        let reason = match event.payload {
            DelegationEventPayload::Granted(_) => None,
            DelegationEventPayload::Revoked { reason } => Some(reason),
        };
        Self {
            stream_position: event.stream_position,
            kind: event_kind_label(event.kind),
            actor_principal_id: event.actor_principal_id,
            resulting_version: event.resulting_version,
            recorded_at_seconds: unix_seconds(event.recorded_at),
            reason,
        }
    }
}

async fn ensure_repository_visible(
    state: &DelegationApiState,
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
            tracing::error!(%error, "Bridge delegation repository visibility query failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn delegation_store_error(error: DelegationStoreError) -> StatusCode {
    tracing::error!(%error, "Bridge delegation query failed");
    StatusCode::INTERNAL_SERVER_ERROR
}

fn projection_state(projection: &DelegationProjection, now: SystemTime) -> &'static str {
    match projection.status {
        DelegationProjectionStatus::Revoked => "revoked",
        DelegationProjectionStatus::Active if projection.expires_at <= now => "expired",
        DelegationProjectionStatus::Active => "active",
    }
}

fn action_label(action: DelegatedAction) -> &'static str {
    match action {
        DelegatedAction::RetrieveContext => "retrieve_context",
        DelegatedAction::Analyze => "analyze",
        DelegatedAction::ClaimTask => "claim_task",
        DelegatedAction::WorkTask => "work_task",
        DelegatedAction::CreateCandidateKnowledge => "create_candidate_knowledge",
        DelegatedAction::RunValidation => "run_validation",
        DelegatedAction::ReportEvidence => "report_evidence",
    }
}

fn event_kind_label(kind: DelegationEventKind) -> &'static str {
    match kind {
        DelegationEventKind::Granted => "granted",
        DelegationEventKind::Revoked => "revoked",
    }
}

fn unix_seconds(timestamp: SystemTime) -> Option<u64> {
    timestamp
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

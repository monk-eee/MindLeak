//! Browser-safe read resources for the ADR-0115 human decision queue.
//!
//! Item 5 requires an escalation to appear in the Bridge's human queue rather
//! than stay hidden in agent logs. This module is deliberately read-only: the
//! approve/refuse intervention surface item 7 describes is a separate command
//! path with its own rationale and receipt, and a browser GET must never
//! become the thing that resolves an escalation.

use std::{sync::Arc, time::SystemTime};

use ackplane_server::{
    fleet::FleetStore,
    human_decision_store::{
        HumanDecisionListCursor, HumanDecisionProjection, HumanDecisionStatus, HumanDecisionStore,
        HumanDecisionStoreError, SafeBehavior,
    },
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Html,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

const DECISIONS_PAGE: &str = include_str!("../static/decisions.html");
const DEFAULT_LIMIT: i64 = 30;
const MAX_LIMIT: i64 = 100;

#[derive(Clone)]
pub struct HumanDecisionApiState {
    decisions: Arc<HumanDecisionStore>,
    fleet: Arc<FleetStore>,
    tenant_id: Arc<str>,
}

impl HumanDecisionApiState {
    pub fn new(
        decisions: Arc<HumanDecisionStore>,
        fleet: Arc<FleetStore>,
        tenant_id: Arc<str>,
    ) -> Self {
        Self {
            decisions,
            fleet,
            tenant_id,
        }
    }
}

pub fn human_decision_routes(state: HumanDecisionApiState) -> Router {
    Router::new()
        .route("/decisions", get(decisions_page))
        .route(
            "/api/v1/repositories/:repository_id/decisions",
            get(decisions),
        )
        .route(
            "/api/v1/repositories/:repository_id/decisions/:decision_id",
            get(decision),
        )
        .with_state(state)
}

#[derive(Deserialize)]
struct DecisionQuery {
    limit: Option<i64>,
    status: Option<String>,
    after_source_event_position: Option<u64>,
    after_decision_id: Option<String>,
}

#[derive(Serialize)]
struct DecisionListResponse {
    entries: Vec<DecisionResponse>,
    effective_limit: i64,
    next_after: Option<DecisionCursorResponse>,
}

#[derive(Serialize)]
struct DecisionCursorResponse {
    source_event_position: u64,
    decision_id: String,
}

#[derive(Serialize)]
struct DecisionResponse {
    decision_id: String,
    proposing_principal_id: String,
    proposed_action: String,
    target: String,
    reason: String,
    context_packet_digest: String,
    evidence_digest: String,
    alternatives: String,
    safe_behavior: &'static str,
    related_delegation_id: Option<String>,
    requested_at_seconds: Option<u64>,
    expires_at_seconds: Option<u64>,
    status: &'static str,
    /// The status a human is actually looking at: a request nobody answered
    /// before its expiry reads `expired`, never `approved` (ADR-0115 item 6).
    state: &'static str,
    version: u32,
    source_event_position: u64,
    resolved_at_seconds: Option<u64>,
    resolved_by_principal_id: Option<String>,
    resolution_rationale: Option<String>,
}

async fn decisions_page() -> Html<&'static str> {
    Html(DECISIONS_PAGE)
}

async fn decisions(
    State(state): State<HumanDecisionApiState>,
    Path(repository_id): Path<String>,
    Query(query): Query<DecisionQuery>,
) -> Result<Json<DecisionListResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let effective_limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let status = parse_status(query.status.as_deref())?;
    let after = parse_decision_cursor(query.after_source_event_position, query.after_decision_id)?;
    let page = state
        .decisions
        .list_page(
            state.tenant_id.as_ref(),
            &repository_id,
            status,
            after.as_ref(),
            effective_limit,
        )
        .await
        .map_err(human_decision_store_error)?;
    let now = SystemTime::now();
    Ok(Json(DecisionListResponse {
        entries: page
            .entries
            .into_iter()
            .map(|projection| DecisionResponse::from_projection(projection, now))
            .collect(),
        effective_limit,
        next_after: page.next_after.map(DecisionCursorResponse::from),
    }))
}

async fn decision(
    State(state): State<HumanDecisionApiState>,
    Path((repository_id, decision_id)): Path<(String, String)>,
) -> Result<Json<DecisionResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let projection = state
        .decisions
        .get(state.tenant_id.as_ref(), &repository_id, &decision_id)
        .await
        .map_err(human_decision_store_error)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(DecisionResponse::from_projection(
        projection,
        SystemTime::now(),
    )))
}

fn parse_status(status: Option<&str>) -> Result<Option<HumanDecisionStatus>, StatusCode> {
    match status {
        None => Ok(None),
        Some("pending") => Ok(Some(HumanDecisionStatus::Pending)),
        Some("approved") => Ok(Some(HumanDecisionStatus::Approved)),
        Some("denied") => Ok(Some(HumanDecisionStatus::Denied)),
        Some(_) => Err(StatusCode::BAD_REQUEST),
    }
}

fn parse_decision_cursor(
    after_source_event_position: Option<u64>,
    after_decision_id: Option<String>,
) -> Result<Option<HumanDecisionListCursor>, StatusCode> {
    match (after_source_event_position, after_decision_id) {
        (None, None) => Ok(None),
        (Some(source_event_position), Some(decision_id))
            if source_event_position > 0
                && i64::try_from(source_event_position).is_ok()
                && !decision_id.is_empty()
                && decision_id.len() <= 256 =>
        {
            Ok(Some(HumanDecisionListCursor {
                source_event_position,
                decision_id,
            }))
        }
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

impl From<HumanDecisionListCursor> for DecisionCursorResponse {
    fn from(cursor: HumanDecisionListCursor) -> Self {
        Self {
            source_event_position: cursor.source_event_position,
            decision_id: cursor.decision_id,
        }
    }
}

impl DecisionResponse {
    fn from_projection(projection: HumanDecisionProjection, now: SystemTime) -> Self {
        let state = projection_state(&projection, now);
        Self {
            decision_id: projection.decision_id,
            proposing_principal_id: projection.proposing_principal_id,
            proposed_action: projection.proposed_action,
            target: projection.target,
            reason: projection.reason,
            context_packet_digest: hex_digest(&projection.context_packet_digest),
            evidence_digest: hex_digest(&projection.evidence_digest),
            alternatives: projection.alternatives,
            safe_behavior: safe_behavior_label(projection.safe_behavior),
            related_delegation_id: projection.related_delegation_id,
            requested_at_seconds: unix_seconds(projection.requested_at),
            expires_at_seconds: unix_seconds(projection.expires_at),
            status: status_label(projection.status),
            state,
            version: projection.version,
            source_event_position: projection.source_event_position,
            resolved_at_seconds: projection.resolved_at.and_then(unix_seconds),
            resolved_by_principal_id: projection.resolved_by_principal_id,
            resolution_rationale: projection.resolution_rationale,
        }
    }
}

async fn ensure_repository_visible(
    state: &HumanDecisionApiState,
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
            tracing::error!(%error, "Bridge human decision repository visibility query failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn human_decision_store_error(error: HumanDecisionStoreError) -> StatusCode {
    tracing::error!(%error, "Bridge human decision query failed");
    StatusCode::INTERNAL_SERVER_ERROR
}

/// A pending request whose expiry has passed is shown as `expired`, never as
/// an approval - waiting is not consent (ADR-0115 item 6). This is derived at
/// read time rather than written back over the durable status.
fn projection_state(projection: &HumanDecisionProjection, now: SystemTime) -> &'static str {
    match projection.status {
        HumanDecisionStatus::Pending if projection.expires_at <= now => "expired",
        HumanDecisionStatus::Pending => "pending",
        HumanDecisionStatus::Approved => "approved",
        HumanDecisionStatus::Denied => "denied",
    }
}

fn status_label(status: HumanDecisionStatus) -> &'static str {
    match status {
        HumanDecisionStatus::Pending => "pending",
        HumanDecisionStatus::Approved => "approved",
        HumanDecisionStatus::Denied => "denied",
    }
}

fn safe_behavior_label(safe_behavior: SafeBehavior) -> &'static str {
    match safe_behavior {
        SafeBehavior::ContinueWithinDelegation => "continue_within_delegation",
        SafeBehavior::CheckpointAndPause => "checkpoint_and_pause",
        SafeBehavior::Drain => "drain",
        SafeBehavior::Refuse => "refuse",
    }
}

fn hex_digest(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unix_seconds(timestamp: SystemTime) -> Option<u64> {
    timestamp
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn projection(status: HumanDecisionStatus, expires_at: SystemTime) -> HumanDecisionProjection {
        HumanDecisionProjection {
            decision_id: "decision-1".to_string(),
            proposing_principal_id: "principal:agent".to_string(),
            proposed_action: "action:export".to_string(),
            target: "artifact:report".to_string(),
            reason: "outside the delegation envelope".to_string(),
            context_packet_digest: vec![0xab; 32],
            evidence_digest: vec![0xcd; 32],
            alternatives: "narrow the export".to_string(),
            safe_behavior: SafeBehavior::CheckpointAndPause,
            related_delegation_id: None,
            requested_at: SystemTime::UNIX_EPOCH + Duration::from_secs(1_000),
            expires_at,
            status,
            version: 1,
            source_event_position: 1,
            resolved_at: None,
            resolved_by_principal_id: None,
            resolution_rationale: None,
        }
    }

    /// ADR-0115 item 6: no response is not an approval. A pending request that
    /// nobody answered before its expiry must never read as approved, and the
    /// durable status must stay `pending` rather than being rewritten.
    #[test]
    fn an_unanswered_request_past_its_expiry_reads_as_expired_not_approved() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(5_000);
        let expired = projection(
            HumanDecisionStatus::Pending,
            SystemTime::UNIX_EPOCH + Duration::from_secs(2_000),
        );

        let response = DecisionResponse::from_projection(expired, now);

        assert_eq!(response.state, "expired");
        assert_eq!(response.status, "pending");
    }

    #[test]
    fn a_pending_request_inside_its_window_reads_as_pending() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(5_000);
        let pending = projection(
            HumanDecisionStatus::Pending,
            SystemTime::UNIX_EPOCH + Duration::from_secs(9_000),
        );

        let response = DecisionResponse::from_projection(pending, now);

        assert_eq!(response.state, "pending");
        assert_eq!(response.status, "pending");
    }

    /// The queue must show the digests item 5 names, and a digest is only
    /// useful to a reviewer as hex - raw bytes would serialise as a number
    /// array nobody can compare against an evidence bundle.
    #[test]
    fn digests_are_rendered_as_hex_for_a_reviewer() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(5_000);
        let response = DecisionResponse::from_projection(
            projection(
                HumanDecisionStatus::Pending,
                SystemTime::UNIX_EPOCH + Duration::from_secs(9_000),
            ),
            now,
        );

        assert_eq!(response.context_packet_digest, "ab".repeat(32));
        assert_eq!(response.evidence_digest, "cd".repeat(32));
    }

    #[test]
    fn an_unknown_status_filter_is_refused_rather_than_ignored() {
        assert_eq!(parse_status(Some("maybe")), Err(StatusCode::BAD_REQUEST));
        assert_eq!(parse_status(None), Ok(None));
        assert_eq!(
            parse_status(Some("pending")),
            Ok(Some(HumanDecisionStatus::Pending))
        );
    }

    /// A half-specified cursor cannot address a stable position, so it is
    /// refused rather than silently restarting the page from the beginning.
    #[test]
    fn a_half_specified_cursor_is_refused() {
        assert_eq!(
            parse_decision_cursor(Some(1), None),
            Err(StatusCode::BAD_REQUEST)
        );
        assert_eq!(
            parse_decision_cursor(None, Some("decision-1".to_string())),
            Err(StatusCode::BAD_REQUEST)
        );
        assert_eq!(
            parse_decision_cursor(Some(0), Some("decision-1".to_string())),
            Err(StatusCode::BAD_REQUEST)
        );
        assert_eq!(parse_decision_cursor(None, None), Ok(None));
    }
}

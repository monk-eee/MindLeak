//! Read-only HTTP routes for the Industrial Evidence Board.
//!
//! This module owns its routes and state so the shared Bridge entry point only
//! needs to merge it. Every read checks tenant-scoped repository visibility
//! before it reaches the Evidence store.

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ackplane_server::{
    evidence_store::{
        ConformanceCursor, ConformanceStoreError, EvidenceCursor, EvidenceStoreError,
    },
    fleet::FleetStore,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::evidence::{page_limit, BridgeEvidenceStore, ConformanceView, EvidenceView};

/// Dependencies injected by the Bridge entry point when it merges Evidence
/// Board routes into its application.
#[derive(Clone)]
pub struct EvidenceApiState {
    pub evidence: Arc<BridgeEvidenceStore>,
    pub fleet: Arc<FleetStore>,
    pub tenant_id: Arc<str>,
}

/// Builds the read-only Evidence Board sub-router.
pub fn evidence_routes(state: EvidenceApiState) -> Router {
    Router::new()
        .route(
            "/api/v1/repositories/:repository_id/tasks/:task_id/evidence",
            get(task_evidence),
        )
        .route(
            "/api/v1/repositories/:repository_id/tasks/:task_id/conformance",
            get(task_conformance),
        )
        .with_state(state)
}

#[derive(Deserialize)]
struct EvidencePageQuery {
    limit: Option<u32>,
    cursor: Option<String>,
}

#[derive(Serialize)]
struct EvidencePageResponse {
    entries: Vec<EvidenceEntryResponse>,
    effective_limit: i64,
    next_cursor: Option<String>,
}

#[derive(Serialize)]
struct EvidenceEntryResponse {
    evidence_id: String,
    task_id: String,
    kind: &'static str,
    source_ref: String,
    content_digest_hex: String,
    observed_at_seconds: Option<u64>,
    reported_agent_session_id: String,
    recorded_by: String,
    recorded_at_seconds: Option<u64>,
    receipt_id: Option<String>,
}

impl From<EvidenceView> for EvidenceEntryResponse {
    fn from(view: EvidenceView) -> Self {
        Self {
            evidence_id: view.evidence_id,
            task_id: view.task_id,
            kind: view.kind,
            source_ref: view.source_ref,
            content_digest_hex: view.content_digest_hex,
            observed_at_seconds: unix_seconds(view.observed_at),
            reported_agent_session_id: view.reported_agent_session_id,
            recorded_by: view.recorded_by,
            recorded_at_seconds: unix_seconds(view.recorded_at),
            receipt_id: view.receipt_id,
        }
    }
}

#[derive(Serialize)]
struct ConformancePageResponse {
    entries: Vec<ConformanceEntryResponse>,
    effective_limit: i64,
    next_cursor: Option<String>,
}

#[derive(Serialize)]
struct ConformanceEntryResponse {
    conformance_id: String,
    task_id: String,
    evidence_id: String,
    verdict: &'static str,
    finding_count: u32,
    findings_digest_hex: String,
    review_state: &'static str,
    reported_checked_at_seconds: Option<u64>,
    evaluated_by: String,
    recorded_at_seconds: Option<u64>,
}

impl From<ConformanceView> for ConformanceEntryResponse {
    fn from(view: ConformanceView) -> Self {
        Self {
            conformance_id: view.conformance_id,
            task_id: view.task_id,
            evidence_id: view.evidence_id,
            verdict: view.verdict,
            finding_count: view.finding_count,
            findings_digest_hex: view.findings_digest_hex,
            review_state: view.review_state,
            reported_checked_at_seconds: unix_seconds(view.reported_checked_at),
            evaluated_by: view.evaluated_by,
            recorded_at_seconds: unix_seconds(view.recorded_at),
        }
    }
}

async fn task_evidence(
    State(state): State<EvidenceApiState>,
    Path((repository_id, task_id)): Path<(String, String)>,
    Query(query): Query<EvidencePageQuery>,
) -> Result<Json<EvidencePageResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let cursor =
        parse_cursor(query.cursor.as_deref())?.map(|(recorded_at, evidence_id)| EvidenceCursor {
            recorded_at,
            evidence_id,
        });
    let effective_limit = page_limit(query.limit);
    let page = state
        .evidence
        .task_evidence(
            state.tenant_id.as_ref(),
            &repository_id,
            &task_id,
            cursor.as_ref(),
            query.limit,
        )
        .await
        .map_err(evidence_store_error)?;
    Ok(Json(EvidencePageResponse {
        entries: page
            .entries
            .into_iter()
            .map(EvidenceView::from)
            .map(EvidenceEntryResponse::from)
            .collect(),
        effective_limit,
        next_cursor: page
            .next_cursor
            .map(|cursor| encode_cursor(cursor.recorded_at, cursor.evidence_id))
            .transpose()?,
    }))
}

async fn task_conformance(
    State(state): State<EvidenceApiState>,
    Path((repository_id, task_id)): Path<(String, String)>,
    Query(query): Query<EvidencePageQuery>,
) -> Result<Json<ConformancePageResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let cursor = parse_cursor(query.cursor.as_deref())?.map(|(recorded_at, conformance_id)| {
        ConformanceCursor {
            recorded_at,
            conformance_id,
        }
    });
    let effective_limit = page_limit(query.limit);
    let page = state
        .evidence
        .task_conformance(
            state.tenant_id.as_ref(),
            &repository_id,
            &task_id,
            cursor.as_ref(),
            query.limit,
        )
        .await
        .map_err(conformance_store_error)?;
    Ok(Json(ConformancePageResponse {
        entries: page
            .entries
            .into_iter()
            .map(ConformanceView::from)
            .map(ConformanceEntryResponse::from)
            .collect(),
        effective_limit,
        next_cursor: page
            .next_cursor
            .map(|cursor| encode_cursor(cursor.recorded_at, cursor.conformance_id))
            .transpose()?,
    }))
}

async fn ensure_repository_visible(
    state: &EvidenceApiState,
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
            tracing::error!(%error, "Bridge Evidence repository visibility query failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn evidence_store_error(error: EvidenceStoreError) -> StatusCode {
    match error {
        EvidenceStoreError::InvalidTaskId | EvidenceStoreError::InvalidCursor => {
            StatusCode::BAD_REQUEST
        }
        EvidenceStoreError::Database(error) => {
            tracing::error!(%error, "Bridge Evidence query failed");
            StatusCode::INTERNAL_SERVER_ERROR
        }
        EvidenceStoreError::InvalidSourceRef
        | EvidenceStoreError::InvalidDigest
        | EvidenceStoreError::InvalidIdentity
        | EvidenceStoreError::InvalidIdempotencyKey
        | EvidenceStoreError::IdempotencyConflict
        | EvidenceStoreError::UnknownStoredKind(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn conformance_store_error(error: ConformanceStoreError) -> StatusCode {
    match error {
        ConformanceStoreError::InvalidTaskId | ConformanceStoreError::InvalidEvidenceId => {
            StatusCode::BAD_REQUEST
        }
        ConformanceStoreError::Database(error) => {
            tracing::error!(%error, "Bridge conformance query failed");
            StatusCode::INTERNAL_SERVER_ERROR
        }
        ConformanceStoreError::InvalidFindingsDigest
        | ConformanceStoreError::InvalidEvaluator
        | ConformanceStoreError::InvalidIdempotencyKey
        | ConformanceStoreError::MissingEvidence
        | ConformanceStoreError::EvidenceTaskMismatch
        | ConformanceStoreError::EvidenceProducerMismatch
        | ConformanceStoreError::IdempotencyConflict
        | ConformanceStoreError::UnknownStoredVerdict(_)
        | ConformanceStoreError::UnknownStoredReviewState(_)
        | ConformanceStoreError::InconsistentReviewState
        | ConformanceStoreError::InvalidStoredFindingCount(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn unix_seconds(timestamp: SystemTime) -> Option<u64> {
    timestamp
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

fn encode_cursor(recorded_at: SystemTime, record_id: String) -> Result<String, StatusCode> {
    let elapsed = recorded_at
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(format!(
        "{}.{:09}:{}",
        elapsed.as_secs(),
        elapsed.subsec_nanos(),
        record_id
    ))
}

fn parse_cursor(raw: Option<&str>) -> Result<Option<(SystemTime, String)>, StatusCode> {
    let Some(raw) = raw.filter(|raw| !raw.is_empty()) else {
        return Ok(None);
    };
    let (timestamp, record_id) = raw.split_once(':').ok_or(StatusCode::BAD_REQUEST)?;
    let (seconds, nanos) = timestamp.split_once('.').ok_or(StatusCode::BAD_REQUEST)?;
    if nanos.len() != 9
        || !nanos.bytes().all(|byte| byte.is_ascii_digit())
        || record_id.is_empty()
        || record_id.len() > 256
        || !record_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let seconds = seconds
        .parse::<u64>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let nanos = nanos.parse::<u32>().map_err(|_| StatusCode::BAD_REQUEST)?;
    let timestamp = UNIX_EPOCH
        .checked_add(Duration::from_secs(seconds))
        .and_then(|timestamp| timestamp.checked_add(Duration::from_nanos(u64::from(nanos))))
        .ok_or(StatusCode::BAD_REQUEST)?;
    Ok(Some((timestamp, record_id.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trip_preserves_server_order_and_record_identity() {
        let timestamp = UNIX_EPOCH + Duration::from_secs(1_234) + Duration::from_nanos(567);
        let encoded = encode_cursor(timestamp, "evidence-abc123".to_string())
            .expect("a post-epoch timestamp encodes");

        assert_eq!(
            parse_cursor(Some(&encoded)),
            Ok(Some((timestamp, "evidence-abc123".to_string())))
        );
    }

    #[test]
    fn cursor_rejects_incomplete_or_untyped_values() {
        for raw in [
            "1234.000000000",
            "1234:evidence-abc123",
            "1234.0:evidence-abc123",
            "1234.1000000000:evidence-abc123",
            "1234.000000000:source/body",
        ] {
            assert_eq!(parse_cursor(Some(raw)), Err(StatusCode::BAD_REQUEST));
        }
    }
}

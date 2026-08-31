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
        ConformanceCursor, ConformanceHistoryFilter, ConformanceReviewState, ConformanceStoreError,
        EvidenceCursor, EvidenceStoreError,
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

mod board;
mod detail;
mod export;
mod page;

const DEFAULT_EVIDENCE_STALE_AFTER: Duration = Duration::from_secs(15 * 60);

/// Dependencies injected by the Bridge entry point when it merges Evidence
/// Board routes into its application.
#[derive(Clone)]
pub struct EvidenceApiState {
    pub evidence: Arc<BridgeEvidenceStore>,
    pub fleet: Arc<FleetStore>,
    pub tenant_id: Arc<str>,
    pub stale_after: Duration,
}

impl EvidenceApiState {
    pub fn new(
        evidence: Arc<BridgeEvidenceStore>,
        fleet: Arc<FleetStore>,
        tenant_id: Arc<str>,
    ) -> Self {
        Self {
            evidence,
            fleet,
            tenant_id,
            stale_after: DEFAULT_EVIDENCE_STALE_AFTER,
        }
    }
}

/// Builds the read-only Evidence Board sub-router.
pub fn evidence_routes(state: EvidenceApiState) -> Router {
    Router::new()
        .route("/evidence", get(page::evidence_page))
        .route(
            "/api/v1/repositories/:repository_id/tasks/:task_id/evidence",
            get(task_evidence),
        )
        .route(
            "/api/v1/repositories/:repository_id/tasks/:task_id/evidence/:evidence_id",
            get(detail::evidence_detail),
        )
        .route(
            "/api/v1/repositories/:repository_id/tasks/:task_id/conformance",
            get(task_conformance),
        )
        .route(
            "/api/v1/repositories/:repository_id/tasks/:task_id/conformance/:conformance_id",
            get(detail::conformance_detail),
        )
        .route(
            "/api/v1/repositories/:repository_id/tasks/:task_id/evidence-board",
            get(board::evidence_board),
        )
        .route(
            "/api/v1/repositories/:repository_id/tasks/:task_id/evidence-board/export",
            get(export::evidence_export),
        )
        .with_state(state)
}

#[derive(Deserialize)]
struct EvidencePageQuery {
    limit: Option<u32>,
    cursor: Option<String>,
    agent_id: Option<String>,
    review_state: Option<String>,
}

#[derive(Serialize)]
pub(super) struct EvidencePageResponse {
    pub(super) entries: Vec<EvidenceEntryResponse>,
    pub(super) effective_limit: i64,
    pub(super) next_cursor: Option<String>,
}

#[derive(Serialize)]
pub(super) struct EvidenceEntryResponse {
    pub(super) evidence_id: String,
    pub(super) task_id: String,
    pub(super) kind: &'static str,
    pub(super) source_ref: String,
    pub(super) content_digest_hex: String,
    pub(super) observed_at_seconds: Option<u64>,
    pub(super) reported_constitution_version: Option<String>,
    pub(super) recorded_by: String,
    pub(super) recorded_at_seconds: Option<u64>,
    pub(super) receipt_id: Option<String>,
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
            reported_constitution_version: view.reported_constitution_version,
            recorded_by: view.recorded_by,
            recorded_at_seconds: unix_seconds(view.recorded_at),
            receipt_id: view.receipt_id,
        }
    }
}

#[derive(Serialize)]
pub(super) struct ConformancePageResponse {
    pub(super) entries: Vec<ConformanceEntryResponse>,
    pub(super) effective_limit: i64,
    pub(super) next_cursor: Option<String>,
}

#[derive(Serialize)]
pub(super) struct ConformanceEntryResponse {
    pub(super) conformance_id: String,
    pub(super) task_id: String,
    pub(super) evidence_id: String,
    pub(super) verdict: &'static str,
    pub(super) finding_count: u32,
    pub(super) findings_digest_hex: String,
    pub(super) finding_codes: Vec<String>,
    pub(super) review_state: &'static str,
    pub(super) reported_checked_at_seconds: Option<u64>,
    pub(super) evaluated_by: String,
    pub(super) reported_constitution_version: Option<String>,
    pub(super) recorded_at_seconds: Option<u64>,
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
            finding_codes: view.finding_codes,
            review_state: view.review_state,
            reported_checked_at_seconds: unix_seconds(view.reported_checked_at),
            evaluated_by: view.evaluated_by,
            reported_constitution_version: view.reported_constitution_version,
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
            query.agent_id.as_deref(),
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
    let review_state = parse_review_state(query.review_state.as_deref())?;
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
            ConformanceHistoryFilter {
                agent_id: query.agent_id.as_deref(),
                review_state,
            },
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

pub(super) async fn ensure_repository_visible(
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

pub(super) fn evidence_store_error(error: EvidenceStoreError) -> StatusCode {
    match error {
        EvidenceStoreError::InvalidTaskId
        | EvidenceStoreError::InvalidEvidenceId
        | EvidenceStoreError::InvalidCursor
        | EvidenceStoreError::InvalidIdentity => StatusCode::BAD_REQUEST,
        EvidenceStoreError::Database(error) => {
            tracing::error!(%error, "Bridge Evidence query failed");
            StatusCode::INTERNAL_SERVER_ERROR
        }
        EvidenceStoreError::InvalidSourceRef
        | EvidenceStoreError::InvalidDigest
        | EvidenceStoreError::InvalidConstitutionVersion
        | EvidenceStoreError::InvalidIdempotencyKey
        | EvidenceStoreError::IdempotencyConflict
        | EvidenceStoreError::UnknownStoredKind(_) => StatusCode::INTERNAL_SERVER_ERROR,
        EvidenceStoreError::PoolExhausted(error) => {
            tracing::error!(%error, "Bridge Evidence store connection pool exhausted");
            StatusCode::SERVICE_UNAVAILABLE
        }
        EvidenceStoreError::SigningKey(error) => {
            tracing::error!(%error, "Bridge Evidence signing key resolution failed");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

pub(super) fn conformance_store_error(error: ConformanceStoreError) -> StatusCode {
    match error {
        ConformanceStoreError::InvalidTaskId
        | ConformanceStoreError::InvalidEvidenceId
        | ConformanceStoreError::InvalidEvaluator => StatusCode::BAD_REQUEST,
        ConformanceStoreError::Database(error) => {
            tracing::error!(%error, "Bridge conformance query failed");
            StatusCode::INTERNAL_SERVER_ERROR
        }
        ConformanceStoreError::InvalidFindingsDigest
        | ConformanceStoreError::TooManyFindingCodes
        | ConformanceStoreError::DuplicateFindingCode
        | ConformanceStoreError::FindingCodeCountExceedsFindingCount
        | ConformanceStoreError::InvalidConstitutionVersion
        | ConformanceStoreError::InvalidIdempotencyKey
        | ConformanceStoreError::MissingEvidence
        | ConformanceStoreError::EvidenceTaskMismatch
        | ConformanceStoreError::EvidenceProducerMismatch
        | ConformanceStoreError::IdempotencyConflict
        | ConformanceStoreError::UnknownStoredVerdict(_)
        | ConformanceStoreError::UnknownStoredReviewState(_)
        | ConformanceStoreError::InconsistentReviewState
        | ConformanceStoreError::InvalidStoredFindingCount(_) => StatusCode::INTERNAL_SERVER_ERROR,
        ConformanceStoreError::UnknownStoredFindingCode(_) => StatusCode::INTERNAL_SERVER_ERROR,
        ConformanceStoreError::PoolExhausted(error) => {
            tracing::error!(%error, "Bridge conformance store connection pool exhausted");
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

pub(super) fn unix_seconds(timestamp: SystemTime) -> Option<u64> {
    timestamp
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

pub(super) fn encode_cursor(
    recorded_at: SystemTime,
    record_id: String,
) -> Result<String, StatusCode> {
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

pub(super) fn parse_cursor(raw: Option<&str>) -> Result<Option<(SystemTime, String)>, StatusCode> {
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

pub(super) fn parse_review_state(
    raw: Option<&str>,
) -> Result<Option<ConformanceReviewState>, StatusCode> {
    raw.map(|value| ConformanceReviewState::from_label(value).ok_or(StatusCode::BAD_REQUEST))
        .transpose()
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

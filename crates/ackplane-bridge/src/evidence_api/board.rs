//! Aggregate task view for the Industrial Evidence Board.

use std::time::SystemTime;

use ackplane_server::evidence_store::{ConformanceCursor, EvidenceCursor};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::evidence::{
    evidence_board_status, page_limit, ConformanceView, EvidenceDataState, EvidenceFreshness,
    EvidenceReviewState, EvidenceView,
};

use super::{
    conformance_store_error, encode_cursor, ensure_repository_visible, evidence_store_error,
    parse_cursor, unix_seconds, ConformanceEntryResponse, ConformancePageResponse,
    EvidenceApiState, EvidenceEntryResponse, EvidencePageResponse,
};

#[derive(Deserialize)]
pub(super) struct EvidenceBoardQuery {
    limit: Option<u32>,
    evidence_cursor: Option<String>,
    conformance_cursor: Option<String>,
}

#[derive(Serialize)]
pub(super) struct EvidenceBoardResponse {
    evidence: EvidencePageResponse,
    conformance: ConformancePageResponse,
    status: EvidenceBoardStatusResponse,
}

#[derive(Serialize)]
struct EvidenceBoardStatusResponse {
    evidence: &'static str,
    conformance: &'static str,
    review: &'static str,
    freshness: &'static str,
    latest_recorded_at_seconds: Option<u64>,
}

pub(super) async fn evidence_board(
    State(state): State<EvidenceApiState>,
    Path((repository_id, task_id)): Path<(String, String)>,
    Query(query): Query<EvidenceBoardQuery>,
) -> Result<Json<EvidenceBoardResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let evidence_cursor =
        parse_cursor(query.evidence_cursor.as_deref())?.map(|(recorded_at, evidence_id)| {
            EvidenceCursor {
                recorded_at,
                evidence_id,
            }
        });
    let conformance_cursor =
        parse_cursor(query.conformance_cursor.as_deref())?.map(|(recorded_at, conformance_id)| {
            ConformanceCursor {
                recorded_at,
                conformance_id,
            }
        });
    let effective_limit = page_limit(query.limit);
    let evidence_page = state
        .evidence
        .task_evidence(
            state.tenant_id.as_ref(),
            &repository_id,
            &task_id,
            evidence_cursor.as_ref(),
            query.limit,
        )
        .await
        .map_err(evidence_store_error)?;
    let conformance_page = state
        .evidence
        .task_conformance(
            state.tenant_id.as_ref(),
            &repository_id,
            &task_id,
            conformance_cursor.as_ref(),
            query.limit,
        )
        .await
        .map_err(conformance_store_error)?;
    let evidence_has_next_page = evidence_page.next_cursor.is_some();
    let conformance_has_next_page = conformance_page.next_cursor.is_some();
    let evidence_entries = evidence_page
        .entries
        .into_iter()
        .map(EvidenceView::from)
        .collect::<Vec<_>>();
    let conformance_entries = conformance_page
        .entries
        .into_iter()
        .map(ConformanceView::from)
        .collect::<Vec<_>>();
    let status = evidence_board_status(
        &evidence_entries,
        evidence_has_next_page,
        &conformance_entries,
        conformance_has_next_page,
        SystemTime::now(),
        state.stale_after,
    );

    Ok(Json(EvidenceBoardResponse {
        evidence: EvidencePageResponse {
            entries: evidence_entries
                .into_iter()
                .map(EvidenceEntryResponse::from)
                .collect(),
            effective_limit,
            next_cursor: evidence_page
                .next_cursor
                .map(|cursor| encode_cursor(cursor.recorded_at, cursor.evidence_id))
                .transpose()?,
        },
        conformance: ConformancePageResponse {
            entries: conformance_entries
                .into_iter()
                .map(ConformanceEntryResponse::from)
                .collect(),
            effective_limit,
            next_cursor: conformance_page
                .next_cursor
                .map(|cursor| encode_cursor(cursor.recorded_at, cursor.conformance_id))
                .transpose()?,
        },
        status: EvidenceBoardStatusResponse {
            evidence: data_state_label(status.evidence),
            conformance: data_state_label(status.conformance),
            review: review_state_label(status.review),
            freshness: freshness_label(status.freshness),
            latest_recorded_at_seconds: status.latest_recorded_at.and_then(unix_seconds),
        },
    }))
}

fn data_state_label(state: EvidenceDataState) -> &'static str {
    match state {
        EvidenceDataState::Missing => "missing",
        EvidenceDataState::Partial => "partial",
        EvidenceDataState::Complete => "complete",
    }
}

fn review_state_label(state: EvidenceReviewState) -> &'static str {
    match state {
        EvidenceReviewState::Missing => "missing",
        EvidenceReviewState::Ready => "ready",
        EvidenceReviewState::Pending => "pending",
        EvidenceReviewState::Blocked => "blocked",
    }
}

fn freshness_label(state: EvidenceFreshness) -> &'static str {
    match state {
        EvidenceFreshness::Missing => "missing",
        EvidenceFreshness::Fresh => "fresh",
        EvidenceFreshness::Stale => "stale",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_response_uses_stable_browser_labels() {
        let status = EvidenceBoardStatusResponse {
            evidence: data_state_label(EvidenceDataState::Partial),
            conformance: data_state_label(EvidenceDataState::Complete),
            review: review_state_label(EvidenceReviewState::Pending),
            freshness: freshness_label(EvidenceFreshness::Stale),
            latest_recorded_at_seconds: Some(123),
        };

        assert_eq!(status.evidence, "partial");
        assert_eq!(status.conformance, "complete");
        assert_eq!(status.review, "pending");
        assert_eq!(status.freshness, "stale");
        assert_eq!(status.latest_recorded_at_seconds, Some(123));
    }
}

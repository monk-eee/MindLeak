//! Explicit scoped detail routes for Evidence Board records.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

use crate::evidence::{ConformanceView, EvidenceView};

use super::{
    conformance_store_error, ensure_repository_visible, evidence_store_error,
    ConformanceEntryResponse, EvidenceApiState, EvidenceEntryResponse,
};

pub(super) async fn evidence_detail(
    State(state): State<EvidenceApiState>,
    Path((repository_id, task_id, evidence_id)): Path<(String, String, String)>,
) -> Result<Json<EvidenceEntryResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    state
        .evidence
        .evidence_detail(
            state.tenant_id.as_ref(),
            &repository_id,
            &task_id,
            &evidence_id,
        )
        .await
        .map_err(evidence_store_error)?
        .map(EvidenceView::from)
        .map(EvidenceEntryResponse::from)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

pub(super) async fn conformance_detail(
    State(state): State<EvidenceApiState>,
    Path((repository_id, task_id, conformance_id)): Path<(String, String, String)>,
) -> Result<Json<ConformanceEntryResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    state
        .evidence
        .conformance_detail(
            state.tenant_id.as_ref(),
            &repository_id,
            &task_id,
            &conformance_id,
        )
        .await
        .map_err(conformance_store_error)?
        .map(ConformanceView::from)
        .map(ConformanceEntryResponse::from)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

//! Design read surface: the paged/filterable design list and one design's
//! detail (its lifecycle decision history and materialization revisions).
//! Split from `mod.rs` to stay under the module-length ratchet -- the
//! mutation side lives in `mutations.rs`.

use ackplane_server::design_store::Design;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};

use super::*;

const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 100;

#[derive(Serialize)]
struct DesignSummary {
    design_id: String,
    title: String,
    summary: String,
    source_version: String,
    lifecycle_state: &'static str,
    constitution_version_id: Option<String>,
    work_task_id: Option<String>,
    evidence_id: Option<String>,
    display_label: Option<String>,
    created_at_seconds: Option<u64>,
    updated_at_seconds: Option<u64>,
}

impl From<Design> for DesignSummary {
    fn from(design: Design) -> Self {
        Self {
            design_id: design.design_id,
            title: design.title,
            summary: design.summary,
            source_version: design.source_version,
            lifecycle_state: lifecycle_state_label(design.lifecycle_state),
            constitution_version_id: design.constitution_version_id,
            work_task_id: design.work_task_id,
            evidence_id: design.evidence_id,
            display_label: design.display_label,
            created_at_seconds: unix_seconds(design.created_at),
            updated_at_seconds: unix_seconds(design.updated_at),
        }
    }
}

#[derive(Deserialize)]
pub(super) struct DesignListQuery {
    page: Option<i64>,
    page_size: Option<i64>,
    lifecycle_state: Option<String>,
}

#[derive(Serialize)]
pub(super) struct DesignListResponse {
    items: Vec<DesignSummary>,
    total: i64,
    page: i64,
    page_size: i64,
}

pub(super) async fn design_list(
    State(state): State<DesignApiState>,
    Path(repository_id): Path<String>,
    Query(query): Query<DesignListQuery>,
) -> Result<Json<DesignListResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let page = query.page.unwrap_or(1);
    if page < 1 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let page_size = query
        .page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let lifecycle_state = match query.lifecycle_state {
        Some(raw) => Some(parse_lifecycle_state(&raw).ok_or(StatusCode::BAD_REQUEST)?),
        None => None,
    };
    let result = state
        .designs
        .list_designs(
            state.tenant_id.as_ref(),
            &repository_id,
            lifecycle_state,
            page,
            page_size,
        )
        .await
        .map_err(design_store_error)?;
    Ok(Json(DesignListResponse {
        items: result.items.into_iter().map(DesignSummary::from).collect(),
        total: result.total,
        page,
        page_size,
    }))
}

#[derive(Serialize)]
struct DesignDecisionResponse {
    sequence_number: i64,
    decision_kind: &'static str,
    actor: String,
    rationale: Option<String>,
    recorded_at_seconds: Option<u64>,
}

#[derive(Serialize)]
pub(super) struct DesignDetailResponse {
    design: DesignSummary,
    decisions: Vec<DesignDecisionResponse>,
    materializations: Vec<MaterializationRevisionResponse>,
}

pub(super) async fn design_detail(
    State(state): State<DesignApiState>,
    Path((repository_id, design_id)): Path<(String, String)>,
) -> Result<Json<DesignDetailResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let design = state
        .designs
        .get_design(state.tenant_id.as_ref(), &repository_id, &design_id)
        .await
        .map_err(design_store_error)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let decisions = state
        .designs
        .list_decisions(state.tenant_id.as_ref(), &repository_id, &design_id)
        .await
        .map_err(design_store_error)?
        .into_iter()
        .map(|decision| DesignDecisionResponse {
            sequence_number: decision.sequence_number,
            decision_kind: lifecycle_state_label(decision.decision_kind),
            actor: decision.actor,
            rationale: decision.rationale,
            recorded_at_seconds: unix_seconds(decision.recorded_at),
        })
        .collect();
    let materializations = state
        .materializations
        .lock()
        .await
        .list_materializations(state.tenant_id.as_ref(), &repository_id, &design_id)
        .await
        .map_err(|error| {
            tracing::error!(%error, "Bridge Design materialization query failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .into_iter()
        .map(MaterializationRevisionResponse::from)
        .collect();
    Ok(Json(DesignDetailResponse {
        design: design.into(),
        decisions,
        materializations,
    }))
}

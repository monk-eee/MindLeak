//! Browser-safe resources for ADR-0121's Industrial Design records and
//! materialization revisions (decisions 3, 4, and 6's read surface, plus
//! ADR-0123's first bounded mutation slice): a paged/filterable design
//! list, one design's detail (its lifecycle decision history and
//! materialization revisions), and three mutations -- propose, record a
//! lifecycle decision, and record a materialization revision. Each mutation
//! carries its own safety in the store itself (idempotent creation,
//! compare-and-swap on the observed lifecycle state, and an idempotency-key
//! conflict check) rather than in a caller identity Bridge does not have,
//! mirroring ADR-0111's `recover` precedent. Broader lifecycle-transition
//! *policy* (which transitions are legal) and any Local-repository-affecting
//! effect remain deferred, per ADR-0123.

use std::{sync::Arc, time::SystemTime};

use ackplane_server::{
    design_materialization_store::{
        MaterializationRevision, MaterializationStore, MaterializationStoreError,
        RecordMaterializationRequest,
    },
    design_store::{
        CreateDesignRequest, Design, DesignLifecycleState, DesignStore, DesignStoreError,
        RecordDecisionRequest,
    },
    fleet::FleetStore,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

const DESIGN_PAGE: &str = include_str!("../static/design.html");
const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 100;

#[derive(Clone)]
pub struct DesignApiState {
    designs: Arc<DesignStore>,
    // `Mutex`-wrapped, not a plain `Arc`, because `record_materialization`
    // takes `&mut self` -- the same reason Bridge's `ClaimStore` handle used
    // to be a `Mutex` before ADR-0143 retired it there (ADR-0111).
    materializations: Arc<Mutex<MaterializationStore>>,
    fleet: Arc<FleetStore>,
    tenant_id: Arc<str>,
}

impl DesignApiState {
    pub fn new(
        designs: Arc<DesignStore>,
        materializations: Arc<Mutex<MaterializationStore>>,
        fleet: Arc<FleetStore>,
        tenant_id: Arc<str>,
    ) -> Self {
        Self {
            designs,
            materializations,
            fleet,
            tenant_id,
        }
    }
}

pub fn design_routes(state: DesignApiState) -> Router {
    Router::new()
        .route("/design", get(design_page))
        .route(
            "/api/v1/repositories/:repository_id/designs",
            get(design_list).post(propose_design),
        )
        .route(
            "/api/v1/repositories/:repository_id/designs/:design_id",
            get(design_detail),
        )
        .route(
            "/api/v1/repositories/:repository_id/designs/:design_id/decisions",
            post(record_design_decision),
        )
        .route(
            "/api/v1/repositories/:repository_id/designs/:design_id/materializations",
            post(record_design_materialization),
        )
        .with_state(state)
}

async fn design_page() -> Html<&'static str> {
    Html(DESIGN_PAGE)
}

async fn ensure_repository_visible(
    state: &DesignApiState,
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
            tracing::error!(%error, "Bridge Design repository visibility query failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn design_store_error(error: DesignStoreError) -> StatusCode {
    tracing::error!(%error, "Bridge Design query failed");
    StatusCode::INTERNAL_SERVER_ERROR
}

/// `create_design`'s own errors, mapped so a bad caller input reads as a
/// caller mistake rather than a server fault: bounded-field violations and a
/// foreign-key violation (an unknown Constitution/Work/Evidence reference)
/// are both the caller's to fix; a same-id-different-content resubmission is
/// a real conflict, matching `create_design`'s own immutability guarantee.
fn propose_design_error(error: DesignStoreError) -> StatusCode {
    match error {
        DesignStoreError::EmptyDesignId
        | DesignStoreError::EmptyTitle
        | DesignStoreError::EmptySourceVersion
        | DesignStoreError::EmptyActor => StatusCode::BAD_REQUEST,
        DesignStoreError::DesignImmutabilityViolation { .. } => StatusCode::CONFLICT,
        DesignStoreError::Database(ref database_error)
            if database_error.code()
                == Some(&tokio_postgres::error::SqlState::FOREIGN_KEY_VIOLATION) =>
        {
            StatusCode::BAD_REQUEST
        }
        other => design_store_error(other),
    }
}

/// `record_decision`'s own errors: `LifecycleStateConflict` is exactly the
/// compare-and-swap rejection ADR-0123 relies on for safety without a
/// caller identity (mirroring `ClaimStore::recover`'s CONFLICT per
/// ADR-0111) -- the operator reloads the design and retries, rather than
/// silently racing another decision.
fn record_decision_error(error: DesignStoreError) -> StatusCode {
    match error {
        DesignStoreError::EmptyActor => StatusCode::BAD_REQUEST,
        DesignStoreError::UnknownDesign { .. } => StatusCode::NOT_FOUND,
        DesignStoreError::LifecycleStateConflict { .. } => StatusCode::CONFLICT,
        other => design_store_error(other),
    }
}

fn record_materialization_error(error: MaterializationStoreError) -> StatusCode {
    match error {
        MaterializationStoreError::InvalidActor
        | MaterializationStoreError::InvalidIdempotencyKey
        | MaterializationStoreError::InvalidRationale
        | MaterializationStoreError::EmptyConstitutionVersionId
        | MaterializationStoreError::TooManyGoalIds
        | MaterializationStoreError::InvalidGoalId
        | MaterializationStoreError::TooManyWorkTaskIds => StatusCode::BAD_REQUEST,
        MaterializationStoreError::IdempotencyConflict { .. } => StatusCode::CONFLICT,
        MaterializationStoreError::Database(ref database_error)
            if database_error.code()
                == Some(&tokio_postgres::error::SqlState::FOREIGN_KEY_VIOLATION) =>
        {
            StatusCode::BAD_REQUEST
        }
        other => {
            tracing::error!(%other, "Bridge Design materialization mutation failed");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

fn lifecycle_state_label(state: DesignLifecycleState) -> &'static str {
    match state {
        DesignLifecycleState::Proposed => "proposed",
        DesignLifecycleState::Accepted => "accepted",
        DesignLifecycleState::Rejected => "rejected",
        DesignLifecycleState::Deferred => "deferred",
        DesignLifecycleState::Retired => "retired",
        DesignLifecycleState::Superseded => "superseded",
        DesignLifecycleState::Materialized => "materialized",
    }
}

fn parse_lifecycle_state(raw: &str) -> Option<DesignLifecycleState> {
    match raw {
        "proposed" => Some(DesignLifecycleState::Proposed),
        "accepted" => Some(DesignLifecycleState::Accepted),
        "rejected" => Some(DesignLifecycleState::Rejected),
        "deferred" => Some(DesignLifecycleState::Deferred),
        "retired" => Some(DesignLifecycleState::Retired),
        "superseded" => Some(DesignLifecycleState::Superseded),
        "materialized" => Some(DesignLifecycleState::Materialized),
        _ => None,
    }
}

fn unix_seconds(timestamp: SystemTime) -> Option<u64> {
    timestamp
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

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
struct DesignListQuery {
    page: Option<i64>,
    page_size: Option<i64>,
    lifecycle_state: Option<String>,
}

#[derive(Serialize)]
struct DesignListResponse {
    items: Vec<DesignSummary>,
    total: i64,
    page: i64,
    page_size: i64,
}

async fn design_list(
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

#[derive(Deserialize)]
struct ProposeDesignRequest {
    design_id: String,
    title: String,
    summary: String,
    source_version: String,
    constitution_version_id: Option<String>,
    work_task_id: Option<String>,
    evidence_id: Option<String>,
    /// No longer authoritative (ADR-0142 clause 4): the recorded `proposed_by`
    /// is always the Bridge's own verified principal (`state.tenant_id`), the
    /// same salted `development_tenant_token` Administration and Work
    /// commands already record. This field, if supplied, is accepted for
    /// backward wire compatibility but is never persisted or trusted as
    /// identity -- narrowing `gaps.d/`'s open display-label follow-up rather
    /// than silently forging an operator-chosen name.
    #[serde(default)]
    proposed_by: Option<String>,
    /// ADR-0142 decision 4: a bounded, optional "who to show in the UI"
    /// string, stored separately from and never substituted for `actor`.
    #[serde(default)]
    display_label: Option<String>,
}

async fn propose_design(
    State(state): State<DesignApiState>,
    Path(repository_id): Path<String>,
    Json(request): Json<ProposeDesignRequest>,
) -> Result<StatusCode, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let _ = request.proposed_by;
    state
        .designs
        .create_design(CreateDesignRequest {
            tenant_id: state.tenant_id.to_string(),
            repository_id,
            design_id: request.design_id,
            title: request.title,
            summary: request.summary,
            source_version: request.source_version,
            constitution_version_id: request.constitution_version_id,
            work_task_id: request.work_task_id,
            evidence_id: request.evidence_id,
            proposed_by: state.tenant_id.to_string(),
            display_label: request.display_label,
        })
        .await
        .map_err(propose_design_error)?;
    Ok(StatusCode::CREATED)
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
struct MaterializationRevisionResponse {
    revision_number: i64,
    actor: String,
    rationale: Option<String>,
    constitution_version_id: String,
    work_task_ids: Vec<String>,
    goal_ids: Vec<String>,
    display_label: Option<String>,
    recorded_at_seconds: Option<u64>,
}

impl From<MaterializationRevision> for MaterializationRevisionResponse {
    fn from(revision: MaterializationRevision) -> Self {
        Self {
            revision_number: revision.revision_number,
            actor: revision.actor,
            rationale: revision.rationale,
            constitution_version_id: revision.constitution_version_id,
            work_task_ids: revision.work_task_ids,
            goal_ids: revision.goal_ids,
            display_label: revision.display_label,
            recorded_at_seconds: unix_seconds(revision.recorded_at),
        }
    }
}

#[derive(Serialize)]
struct DesignDetailResponse {
    design: DesignSummary,
    decisions: Vec<DesignDecisionResponse>,
    materializations: Vec<MaterializationRevisionResponse>,
}

async fn design_detail(
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

#[derive(Deserialize)]
struct RecordDesignDecisionRequest {
    decision_kind: String,
    /// No longer authoritative (ADR-0142 clause 4); see
    /// `ProposeDesignRequest::proposed_by`'s doc comment.
    #[serde(default)]
    actor: Option<String>,
    rationale: Option<String>,
    expected_lifecycle_state: String,
}

async fn record_design_decision(
    State(state): State<DesignApiState>,
    Path((repository_id, design_id)): Path<(String, String)>,
    Json(request): Json<RecordDesignDecisionRequest>,
) -> Result<StatusCode, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let _ = request.actor;
    let decision_kind =
        parse_lifecycle_state(&request.decision_kind).ok_or(StatusCode::BAD_REQUEST)?;
    let expected_lifecycle_state =
        parse_lifecycle_state(&request.expected_lifecycle_state).ok_or(StatusCode::BAD_REQUEST)?;
    state
        .designs
        .record_decision(RecordDecisionRequest {
            tenant_id: state.tenant_id.to_string(),
            repository_id,
            design_id,
            decision_kind,
            actor: state.tenant_id.to_string(),
            rationale: request.rationale,
            expected_lifecycle_state,
        })
        .await
        .map_err(record_decision_error)?;
    Ok(StatusCode::CREATED)
}

#[derive(Deserialize)]
struct RecordDesignMaterializationRequest {
    /// No longer authoritative (ADR-0142 clause 4); see
    /// `ProposeDesignRequest::proposed_by`'s doc comment.
    #[serde(default)]
    actor: Option<String>,
    idempotency_key: String,
    rationale: Option<String>,
    constitution_version_id: String,
    #[serde(default)]
    work_task_ids: Vec<String>,
    #[serde(default)]
    goal_ids: Vec<String>,
    /// ADR-0142 decision 4: a bounded, optional "who to show in the UI"
    /// string, stored separately from and never substituted for `actor`.
    #[serde(default)]
    display_label: Option<String>,
}

async fn record_design_materialization(
    State(state): State<DesignApiState>,
    Path((repository_id, design_id)): Path<(String, String)>,
    Json(request): Json<RecordDesignMaterializationRequest>,
) -> Result<Json<MaterializationRevisionResponse>, StatusCode> {
    ensure_repository_visible(&state, &repository_id).await?;
    let _ = request.actor;
    let revision = state
        .materializations
        .lock()
        .await
        .record_materialization(RecordMaterializationRequest {
            tenant_id: state.tenant_id.to_string(),
            repository_id,
            design_id,
            actor: state.tenant_id.to_string(),
            idempotency_key: request.idempotency_key,
            rationale: request.rationale,
            constitution_version_id: request.constitution_version_id,
            work_task_ids: request.work_task_ids,
            goal_ids: request.goal_ids,
            display_label: request.display_label,
        })
        .await
        .map_err(record_materialization_error)?;
    Ok(Json(MaterializationRevisionResponse::from(revision)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_state_labels_round_trip() {
        for state in [
            DesignLifecycleState::Proposed,
            DesignLifecycleState::Accepted,
            DesignLifecycleState::Rejected,
            DesignLifecycleState::Deferred,
            DesignLifecycleState::Retired,
            DesignLifecycleState::Superseded,
            DesignLifecycleState::Materialized,
        ] {
            let label = lifecycle_state_label(state);
            assert_eq!(parse_lifecycle_state(label), Some(state));
        }
        assert_eq!(parse_lifecycle_state("unknown"), None);
    }

    #[tokio::test]
    async fn design_page_binds_the_interactive_workflow() {
        let Html(body) = design_page().await;

        for required in [
            "id=\"repository-id\"",
            "id=\"lifecycle-state\"",
            "/designs",
            "decision_kind",
            "materializations",
            "id=\"propose-design-id\"",
            "id=\"propose-actor\"",
            "id=\"decision-kind\"",
            "id=\"decision-actor\"",
            "id=\"decision-expected-state\"",
            "id=\"materialization-actor\"",
            "id=\"materialization-idempotency-key\"",
            "method:\"POST\"",
        ] {
            assert!(
                body.contains(required),
                "Design page must retain its {required} workflow binding"
            );
        }
    }

    #[tokio::test]
    async fn design_page_cross_links_constitution_work_and_evidence_references() {
        let Html(body) = design_page().await;

        for required in [
            "id=\"detail-constitution-version\"",
            "id=\"detail-work-task\"",
            "id=\"detail-evidence\"",
            "/constitution?",
            "/work?",
            "/evidence?",
        ] {
            assert!(
                body.contains(required),
                "Design page must retain its {required} reference-linking binding"
            );
        }
    }

    #[test]
    fn propose_design_error_maps_bounded_field_violations_to_bad_request() {
        assert_eq!(
            propose_design_error(DesignStoreError::EmptyTitle),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            propose_design_error(DesignStoreError::DesignImmutabilityViolation {
                design_id: "d".to_string()
            }),
            StatusCode::CONFLICT
        );
    }

    #[test]
    fn record_decision_error_maps_conflict_and_unknown_design() {
        assert_eq!(
            record_decision_error(DesignStoreError::LifecycleStateConflict {
                design_id: "d".to_string(),
                expected: DesignLifecycleState::Proposed,
                actual: DesignLifecycleState::Accepted,
            }),
            StatusCode::CONFLICT
        );
        assert_eq!(
            record_decision_error(DesignStoreError::UnknownDesign {
                design_id: "d".to_string()
            }),
            StatusCode::NOT_FOUND
        );
    }

    #[test]
    fn record_materialization_error_maps_idempotency_conflict() {
        assert_eq!(
            record_materialization_error(MaterializationStoreError::IdempotencyConflict {
                design_id: "d".to_string(),
                idempotency_key: "k".to_string(),
            }),
            StatusCode::CONFLICT
        );
        assert_eq!(
            record_materialization_error(MaterializationStoreError::InvalidActor),
            StatusCode::BAD_REQUEST
        );
    }
}

//! Design mutations: propose a design, record a lifecycle decision, and
//! record a materialization revision. Split from `mod.rs` to stay under
//! the module-length ratchet -- the read side lives in `listing.rs`.

use ackplane_server::{
    design_materialization_store::{MaterializationStoreError, RecordMaterializationRequest},
    design_store::{CreateDesignRequest, RecordDecisionRequest},
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;

use super::*;

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

#[derive(Deserialize)]
pub(super) struct ProposeDesignRequest {
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

pub(super) async fn propose_design(
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

#[derive(Deserialize)]
pub(super) struct RecordDesignDecisionRequest {
    decision_kind: String,
    /// No longer authoritative (ADR-0142 clause 4); see
    /// `ProposeDesignRequest::proposed_by`'s doc comment.
    #[serde(default)]
    actor: Option<String>,
    rationale: Option<String>,
    expected_lifecycle_state: String,
}

pub(super) async fn record_design_decision(
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
pub(super) struct RecordDesignMaterializationRequest {
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

pub(super) async fn record_design_materialization(
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

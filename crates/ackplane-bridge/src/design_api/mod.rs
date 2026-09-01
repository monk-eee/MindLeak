//! Browser-safe resources for ADR-0121's Industrial Design records and
//! materialization revisions (decisions 3, 4, and 6's read surface, plus
//! ADR-0123's first bounded mutation slice). Split into `listing` (the
//! paged/filterable design list and one design's detail) and `mutations`
//! (propose, record a lifecycle decision, record a materialization
//! revision) to stay under the module-length ratchet; each mutation
//! carries its own safety in the store itself (idempotent creation,
//! compare-and-swap on the observed lifecycle state, and an idempotency-key
//! conflict check) rather than in a caller identity Bridge does not have,
//! mirroring ADR-0111's `recover` precedent. Broader lifecycle-transition
//! *policy* (which transitions are legal) and any Local-repository-affecting
//! effect remain deferred, per ADR-0123.

use std::{sync::Arc, time::SystemTime};

use ackplane_server::{
    design_materialization_store::{MaterializationRevision, MaterializationStore},
    design_store::{DesignLifecycleState, DesignStore, DesignStoreError},
    fleet::FleetStore,
};
use axum::{
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Router,
};
use serde::Serialize;

mod listing;
mod mutations;

use listing::{design_detail, design_list};
use mutations::{propose_design, record_design_decision, record_design_materialization};

const DESIGN_PAGE: &str = include_str!("../../static/design.html");

#[derive(Clone)]
pub struct DesignApiState {
    designs: Arc<DesignStore>,
    materializations: Arc<MaterializationStore>,
    fleet: Arc<FleetStore>,
    tenant_id: Arc<str>,
}

impl DesignApiState {
    pub fn new(
        designs: Arc<DesignStore>,
        materializations: Arc<MaterializationStore>,
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

/// Shared by both `listing::design_detail` (a read) and
/// `mutations::record_design_materialization` (its own response), so it
/// lives on the shared parent rather than either sibling.
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
}

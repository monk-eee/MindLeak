//! Industrial design records: a separate authority for Industrial-only
//! design/materialization decisions (ADR-0121 decision 3).
//!
//! A design record is opaque `(tenant_id, repository_id, design_id)` identity
//! plus bounded title/summary/source_version, a closed-vocabulary lifecycle
//! state, and optional references into the Constitution/Work/Evidence
//! domains -- each checked by a real composite foreign key against the
//! referenced table in the SAME tenant and repository. Creation is
//! idempotent (an identical retry no-ops; a different `design_id` reused
//! with different content is refused), matching this crate's established
//! immutable/append-only insert pattern. Every lifecycle transition is a
//! separate, append-only decision-history row; nothing here authorizes WHO
//! may record a transition -- that gate is a future typed command, per the
//! ADR's own text.

use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::db_pool::{PgConnection, PgPool};

const MIGRATION: &str = include_str!("../../migrations/0027_industrial_designs.sql");
const WORK_REFERENCE_MIGRATION: &str =
    include_str!("../../migrations/0031_industrial_design_work_reference.sql");
// ADR-0142 decision 4: a caller may still supply a bounded, optional display
// label, stored separately from and never substituted for the authoritative
// `proposed_by`/actor (gaps.d/design-constitution-display-label-not-stored-
// separately.md). Third and final table of that decision -- constitution_
// proposals and industrial_design_materializations already closed it.
const DISPLAY_LABEL_MIGRATION: &str =
    include_str!("../../migrations/0064_industrial_designs_display_label.sql");

// Dependency migrations: 0027 foreign-keys into `evidence_records` and
// `constitution_publications`, and 0031 foreign-keys into `work_tasks` --
// none of them created by this store's own migration keys. A shared,
// long-lived local dev database already has all three tables from other
// stores' earlier connects, which hid this against a genuinely fresh
// database (`relation "constitution_publications" does not exist`) until a
// throwaway, freshly-created Postgres instance was used to test against.
const EVIDENCE_DEPENDENCY_MIGRATION: &str = include_str!("../../migrations/0014_evidence.sql");
const CONSTITUTION_PUBLICATION_HISTORY_DEPENDENCY_MIGRATION: &str =
    include_str!("../../migrations/0026_constitution_publication_history.sql");
const WORK_DEPENDENCY_MIGRATION: &str = include_str!("../../migrations/0028_work.sql");

#[derive(Debug, thiserror::Error)]
pub enum DesignStoreError {
    #[error("industrial design database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("design store could not obtain a database connection: {0}")]
    PoolExhausted(#[from] deadpool_postgres::PoolError),
    #[error("design_id must not be empty")]
    EmptyDesignId,
    #[error("design title must not be empty")]
    EmptyTitle,
    #[error("design source_version must not be empty")]
    EmptySourceVersion,
    #[error("design decision actor must not be empty")]
    EmptyActor,
    #[error(
        "design {design_id} is already recorded with different content -- a design's identity is immutable at creation"
    )]
    DesignImmutabilityViolation { design_id: String },
    #[error("design {design_id} has a corrupt lifecycle_state value {value}")]
    CorruptLifecycleState { design_id: String, value: i16 },
    #[error("design {design_id} was not found")]
    UnknownDesign { design_id: String },
    #[error(
        "design {design_id} is now {actual:?}, not the {expected:?} last observed -- reload and retry"
    )]
    LifecycleStateConflict {
        design_id: String,
        expected: DesignLifecycleState,
        actual: DesignLifecycleState,
    },
}

/// The closed vocabulary both `industrial_designs.lifecycle_state` and
/// `industrial_design_decisions.decision_kind` share -- one design's current
/// state is always its most recently recorded decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesignLifecycleState {
    Proposed = 1,
    Accepted = 2,
    Rejected = 3,
    Deferred = 4,
    Retired = 5,
    Superseded = 6,
    Materialized = 7,
}

impl TryFrom<i16> for DesignLifecycleState {
    type Error = ();

    fn try_from(value: i16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Proposed),
            2 => Ok(Self::Accepted),
            3 => Ok(Self::Rejected),
            4 => Ok(Self::Deferred),
            5 => Ok(Self::Retired),
            6 => Ok(Self::Superseded),
            7 => Ok(Self::Materialized),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateDesignRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub design_id: String,
    pub title: String,
    pub summary: String,
    pub source_version: String,
    pub constitution_version_id: Option<String>,
    pub work_task_id: Option<String>,
    pub evidence_id: Option<String>,
    pub proposed_by: String,
    /// ADR-0142 decision 4: a bounded, optional "who to show in the UI"
    /// string, stored separately from and never substituted for
    /// `proposed_by`.
    pub display_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordDecisionRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub design_id: String,
    pub decision_kind: DesignLifecycleState,
    pub actor: String,
    pub rationale: Option<String>,
    /// The lifecycle_state the caller last observed. The update below only
    /// lands if the stored row still matches it, so two concurrent
    /// decisions against the same design can never both land silently.
    pub expected_lifecycle_state: DesignLifecycleState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Design {
    pub tenant_id: String,
    pub repository_id: String,
    pub design_id: String,
    pub title: String,
    pub summary: String,
    pub source_version: String,
    pub lifecycle_state: DesignLifecycleState,
    pub constitution_version_id: Option<String>,
    pub work_task_id: Option<String>,
    pub evidence_id: Option<String>,
    pub display_label: Option<String>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesignDecision {
    pub sequence_number: i64,
    pub decision_kind: DesignLifecycleState,
    pub actor: String,
    pub rationale: Option<String>,
    pub recorded_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DesignPage {
    pub items: Vec<Design>,
    pub total: i64,
}

/// Private wire shape used only to compute `content_digest` -- a design's
/// queryable columns are the durable record; this is never stored or read
/// back on its own.
#[derive(serde::Serialize)]
struct DesignIdentityPayload {
    title: String,
    summary: String,
    source_version: String,
    constitution_version_id: Option<String>,
    work_task_id: Option<String>,
    evidence_id: Option<String>,
    // ADR-0142 decision 4: included in identity so a retry with a
    // DIFFERENT display_label under the same design_id is a genuine
    // conflict, matching constitution_proposals/design_materialization's
    // own established precedent -- never silently overwritten.
    display_label: Option<String>,
}

pub struct DesignStore {
    pool: PgPool,
}

impl DesignStore {
    /// Takes a clone of the process's single pool (ADR-0143 decision 1), not
    /// a database URL: a store that resolved its own connection would be
    /// exactly the per-store demand the pool exists to bound.
    pub async fn connect(pool: &PgPool) -> Result<Self, DesignStoreError> {
        let mut connection = pool.get().await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::EVIDENCE,
            EVIDENCE_DEPENDENCY_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::CONSTITUTION_PUBLICATION_HISTORY,
            CONSTITUTION_PUBLICATION_HISTORY_DEPENDENCY_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::WORK,
            WORK_DEPENDENCY_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::INDUSTRIAL_DESIGNS,
            MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::INDUSTRIAL_DESIGN_WORK_REFERENCE,
            WORK_REFERENCE_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::INDUSTRIAL_DESIGNS_DISPLAY_LABEL,
            DISPLAY_LABEL_MIGRATION,
        )
        .await?;
        Ok(Self { pool: pool.clone() })
    }

    /// One checked-out connection, held only for the call that asked for it.
    ///
    /// A caller that opens a transaction keeps this binding alive for the
    /// life of that transaction, which is the one case where holding a
    /// connection across `.await` points is correct rather than accidental.
    async fn connection(&self) -> Result<PgConnection, DesignStoreError> {
        Ok(self.pool.get().await?)
    }
}

mod listing;
mod mutations;

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_id(label: &str) -> String {
        format!("{label}-{}", crate::test_support::uuid_ish())
    }

    fn create_request(
        tenant_id: &str,
        repository_id: &str,
        design_id: &str,
    ) -> CreateDesignRequest {
        CreateDesignRequest {
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            design_id: design_id.to_string(),
            title: "a design".to_string(),
            summary: "a summary".to_string(),
            source_version: "v1".to_string(),
            constitution_version_id: None,
            work_task_id: None,
            evidence_id: None,
            proposed_by: "agent:test".to_string(),
            display_label: None,
        }
    }

    #[tokio::test]
    async fn a_created_design_reads_back_as_proposed() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let design_id = unique_id("design");
        let store = DesignStore::connect(&pool).await.unwrap();

        store
            .create_design(create_request(&tenant_id, &repository_id, &design_id))
            .await
            .expect("creating a design should succeed");

        let design = store
            .get_design(&tenant_id, &repository_id, &design_id)
            .await
            .expect("reading the design should succeed")
            .expect("the design should exist");
        assert_eq!(design.title, "a design");
        assert_eq!(design.lifecycle_state, DesignLifecycleState::Proposed);

        let decisions = store
            .list_decisions(&tenant_id, &repository_id, &design_id)
            .await
            .expect("listing decisions should succeed");
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].sequence_number, 1);
        assert_eq!(decisions[0].decision_kind, DesignLifecycleState::Proposed);
        assert_eq!(decisions[0].actor, "agent:test");
    }

    #[tokio::test]
    async fn a_display_label_stores_separately_from_the_authoritative_proposed_by() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let design_id = unique_id("design");
        let store = DesignStore::connect(&pool).await.unwrap();
        let mut request = create_request(&tenant_id, &repository_id, &design_id);
        request.display_label = Some("Jordan (via Bridge)".to_string());

        store
            .create_design(request)
            .await
            .expect("creating a design should succeed");

        let design = store
            .get_design(&tenant_id, &repository_id, &design_id)
            .await
            .expect("reading the design should succeed")
            .expect("the design should exist");
        assert_eq!(
            design.display_label,
            Some("Jordan (via Bridge)".to_string())
        );

        let decisions = store
            .list_decisions(&tenant_id, &repository_id, &design_id)
            .await
            .expect("listing decisions should succeed");
        assert_eq!(decisions[0].actor, "agent:test");
    }

    #[tokio::test]
    async fn a_retry_with_a_different_display_label_is_a_design_immutability_violation() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let design_id = unique_id("design");
        let store = DesignStore::connect(&pool).await.unwrap();
        let mut first = create_request(&tenant_id, &repository_id, &design_id);
        first.display_label = Some("Jordan".to_string());
        store
            .create_design(first)
            .await
            .expect("the first submission should succeed");

        let mut retry = create_request(&tenant_id, &repository_id, &design_id);
        retry.display_label = Some("Alex".to_string());
        let error = store
            .create_design(retry)
            .await
            .expect_err("a different display_label under the same design_id must conflict");
        assert!(matches!(
            error,
            DesignStoreError::DesignImmutabilityViolation { .. }
        ));
    }

    #[tokio::test]
    async fn get_design_returns_none_for_an_unknown_design() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let store = DesignStore::connect(&pool).await.unwrap();

        let design = store
            .get_design(
                &unique_id("tenant"),
                &unique_id("repository"),
                &unique_id("design"),
            )
            .await
            .expect("reading a missing design should succeed with None");
        assert!(design.is_none());
    }

    #[tokio::test]
    async fn creating_the_same_design_twice_is_an_idempotent_no_op() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let design_id = unique_id("design");
        let store = DesignStore::connect(&pool).await.unwrap();

        store
            .create_design(create_request(&tenant_id, &repository_id, &design_id))
            .await
            .expect("the first create should succeed");
        store
            .create_design(create_request(&tenant_id, &repository_id, &design_id))
            .await
            .expect("an identical retry should be a no-op, not an error");

        let decisions = store
            .list_decisions(&tenant_id, &repository_id, &design_id)
            .await
            .expect("listing decisions should succeed");
        assert_eq!(
            decisions.len(),
            1,
            "the retry must not append a second Proposed row"
        );
    }

    #[tokio::test]
    async fn creating_a_different_design_under_the_same_id_is_rejected() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let design_id = unique_id("design");
        let store = DesignStore::connect(&pool).await.unwrap();

        store
            .create_design(create_request(&tenant_id, &repository_id, &design_id))
            .await
            .expect("the first create should succeed");

        let mut different = create_request(&tenant_id, &repository_id, &design_id);
        different.title = "a completely different title".to_string();
        let result = store.create_design(different).await;

        assert!(matches!(
            result,
            Err(DesignStoreError::DesignImmutabilityViolation { .. })
        ));
    }

    #[tokio::test]
    async fn recording_a_decision_appends_history_and_moves_lifecycle_state() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let design_id = unique_id("design");
        let store = DesignStore::connect(&pool).await.unwrap();
        store
            .create_design(create_request(&tenant_id, &repository_id, &design_id))
            .await
            .expect("creating a design should succeed");

        store
            .record_decision(RecordDecisionRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                design_id: design_id.clone(),
                decision_kind: DesignLifecycleState::Accepted,
                actor: "agent:reviewer".to_string(),
                rationale: Some("looks good".to_string()),
                expected_lifecycle_state: DesignLifecycleState::Proposed,
            })
            .await
            .expect("recording a decision should succeed");

        let design = store
            .get_design(&tenant_id, &repository_id, &design_id)
            .await
            .expect("reading the design should succeed")
            .expect("the design should exist");
        assert_eq!(design.lifecycle_state, DesignLifecycleState::Accepted);

        let decisions = store
            .list_decisions(&tenant_id, &repository_id, &design_id)
            .await
            .expect("listing decisions should succeed");
        assert_eq!(decisions.len(), 2);
        assert_eq!(decisions[1].sequence_number, 2);
        assert_eq!(decisions[1].decision_kind, DesignLifecycleState::Accepted);
        assert_eq!(decisions[1].rationale, Some("looks good".to_string()));
    }

    #[tokio::test]
    async fn recording_a_decision_against_an_unknown_design_is_refused() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let store = DesignStore::connect(&pool).await.unwrap();
        let design_id = unique_id("design");

        let result = store
            .record_decision(RecordDecisionRequest {
                tenant_id: unique_id("tenant"),
                repository_id: unique_id("repository"),
                design_id: design_id.clone(),
                decision_kind: DesignLifecycleState::Accepted,
                actor: "agent:reviewer".to_string(),
                rationale: None,
                expected_lifecycle_state: DesignLifecycleState::Proposed,
            })
            .await;

        assert!(matches!(
            result,
            Err(DesignStoreError::UnknownDesign { design_id: ref id }) if *id == design_id
        ));
    }

    #[tokio::test]
    async fn recording_a_decision_with_a_stale_expected_state_is_refused() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let design_id = unique_id("design");
        let store = DesignStore::connect(&pool).await.unwrap();
        store
            .create_design(create_request(&tenant_id, &repository_id, &design_id))
            .await
            .expect("creating a design should succeed");

        // The design is actually Proposed, but the caller claims to have
        // last observed Accepted -- exactly what a second, slower operator
        // would submit after someone else's decision already landed.
        let result = store
            .record_decision(RecordDecisionRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                design_id: design_id.clone(),
                decision_kind: DesignLifecycleState::Rejected,
                actor: "agent:reviewer".to_string(),
                rationale: None,
                expected_lifecycle_state: DesignLifecycleState::Accepted,
            })
            .await;

        assert!(matches!(
            result,
            Err(DesignStoreError::LifecycleStateConflict {
                expected: DesignLifecycleState::Accepted,
                actual: DesignLifecycleState::Proposed,
                ..
            })
        ));

        // The rejected conflict must not have appended a history row or
        // moved the design's own state.
        let design = store
            .get_design(&tenant_id, &repository_id, &design_id)
            .await
            .expect("reading the design should succeed")
            .expect("the design should exist");
        assert_eq!(design.lifecycle_state, DesignLifecycleState::Proposed);
        let decisions = store
            .list_decisions(&tenant_id, &repository_id, &design_id)
            .await
            .expect("listing decisions should succeed");
        assert_eq!(decisions.len(), 1, "only the original Proposed row");
    }

    #[tokio::test]
    async fn a_reference_to_a_nonexistent_evidence_record_is_refused() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let store = DesignStore::connect(&pool).await.unwrap();
        let mut request = create_request(
            &unique_id("tenant"),
            &unique_id("repository"),
            &unique_id("design"),
        );
        request.evidence_id = Some(unique_id("nonexistent-evidence"));

        let result = store.create_design(request).await;

        assert!(matches!(result, Err(DesignStoreError::Database(_))));
    }

    #[tokio::test]
    async fn a_reference_to_a_nonexistent_work_task_is_refused() {
        let Some(pool) = crate::test_support::test_pool() else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let store = DesignStore::connect(&pool).await.unwrap();
        let mut request = create_request(
            &unique_id("tenant"),
            &unique_id("repository"),
            &unique_id("design"),
        );
        request.work_task_id = Some(unique_id("nonexistent-task"));

        let result = store.create_design(request).await;

        assert!(matches!(result, Err(DesignStoreError::Database(_))));
    }

    #[tokio::test]
    async fn a_reference_to_an_existing_work_task_succeeds() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let task_id = unique_id("task");
        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let work_store =
            crate::work_store::WorkStore::connect(&crate::test_support::gated_test_pool())
                .await
                .unwrap();
        work_store
            .create_task(
                &crate::work_store::NewWorkTask {
                    tenant_id: tenant_id.clone(),
                    repository_id: repository_id.clone(),
                    task_id: task_id.clone(),
                    title: "a work task".to_string(),
                    acceptance: "done when done".to_string(),
                    goal_id: None,
                    declared_paths: vec![],
                    declared_symbols: vec![],
                    published_by: "agent:test".to_string(),
                },
                &unique_id("event"),
                SystemTime::now(),
            )
            .await
            .expect("creating the work task should succeed");

        let store = DesignStore::connect(&pool).await.unwrap();
        let mut request = create_request(&tenant_id, &repository_id, &unique_id("design"));
        request.work_task_id = Some(task_id.clone());
        let design_id = request.design_id.clone();
        store
            .create_design(request)
            .await
            .expect("a design referencing a real work task should succeed");

        let design = store
            .get_design(&tenant_id, &repository_id, &design_id)
            .await
            .expect("reading the design should succeed")
            .expect("the design should exist");
        assert_eq!(design.work_task_id, Some(task_id));
    }
}

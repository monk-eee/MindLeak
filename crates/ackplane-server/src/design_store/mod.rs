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
use tokio_postgres::{Client, NoTls};

const MIGRATION: &str = include_str!("../../migrations/0027_industrial_designs.sql");
const WORK_REFERENCE_MIGRATION: &str =
    include_str!("../../migrations/0031_industrial_design_work_reference.sql");

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
}

pub struct DesignStore {
    client: Client,
}

impl DesignStore {
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (mut client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane design store connection closed with an error");
            }
        });
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::EVIDENCE,
            EVIDENCE_DEPENDENCY_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::CONSTITUTION_PUBLICATION_HISTORY,
            CONSTITUTION_PUBLICATION_HISTORY_DEPENDENCY_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::WORK,
            WORK_DEPENDENCY_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::INDUSTRIAL_DESIGNS,
            MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::INDUSTRIAL_DESIGN_WORK_REFERENCE,
            WORK_REFERENCE_MIGRATION,
        )
        .await?;
        Ok(Self { client })
    }

    /// Propose a design: an idempotent-checked insert of the design row plus
    /// its first (`Proposed`) decision-history row, in one transaction. An
    /// identical retry (same identity fields) is a no-op; the same
    /// `design_id` reused with different content is refused.
    pub async fn create_design(
        &mut self,
        request: CreateDesignRequest,
    ) -> Result<(), DesignStoreError> {
        if request.design_id.trim().is_empty() {
            return Err(DesignStoreError::EmptyDesignId);
        }
        if request.title.trim().is_empty() {
            return Err(DesignStoreError::EmptyTitle);
        }
        if request.source_version.trim().is_empty() {
            return Err(DesignStoreError::EmptySourceVersion);
        }
        if request.proposed_by.trim().is_empty() {
            return Err(DesignStoreError::EmptyActor);
        }

        let payload = DesignIdentityPayload {
            title: request.title.clone(),
            summary: request.summary.clone(),
            source_version: request.source_version.clone(),
            constitution_version_id: request.constitution_version_id.clone(),
            work_task_id: request.work_task_id.clone(),
            evidence_id: request.evidence_id.clone(),
        };
        let payload_bytes =
            serde_json::to_vec(&payload).expect("DesignIdentityPayload always serializes");
        let content_digest = Sha256::digest(&payload_bytes).to_vec();

        let transaction = self.client.transaction().await?;
        let existing = transaction
            .query_opt(
                "SELECT content_digest FROM industrial_designs \
                 WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.design_id,
                ],
            )
            .await?;
        if let Some(row) = existing {
            let stored_digest: Vec<u8> = row.get(0);
            transaction.commit().await?;
            if stored_digest == content_digest {
                return Ok(());
            }
            return Err(DesignStoreError::DesignImmutabilityViolation {
                design_id: request.design_id.clone(),
            });
        }

        transaction
            .execute(
                "INSERT INTO industrial_designs \
                 (tenant_id, repository_id, design_id, title, summary, source_version, \
                  lifecycle_state, constitution_version_id, work_task_id, evidence_id, \
                  content_digest) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.design_id,
                    &request.title,
                    &request.summary,
                    &request.source_version,
                    &(DesignLifecycleState::Proposed as i16),
                    &request.constitution_version_id,
                    &request.work_task_id,
                    &request.evidence_id,
                    &content_digest,
                ],
            )
            .await?;
        transaction
            .execute(
                "INSERT INTO industrial_design_decisions \
                 (tenant_id, repository_id, design_id, sequence_number, decision_kind, actor, \
                  rationale) \
                 VALUES ($1, $2, $3, 1, $4, $5, NULL)",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.design_id,
                    &(DesignLifecycleState::Proposed as i16),
                    &request.proposed_by,
                ],
            )
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Append one decision-history row and move the design's own
    /// `lifecycle_state` to match, in one transaction. The guarded UPDATE
    /// runs first: it only matches a row whose `lifecycle_state` still
    /// equals `expected_lifecycle_state`, so two concurrent decisions
    /// against the same design can never both land -- the loser gets
    /// `LifecycleStateConflict` (an unknown `design_id` gets `UnknownDesign`
    /// instead), mirroring `ClaimStore::recover`'s own compare-and-swap
    /// (ADR-0111). Only once that guarded update actually lands does the
    /// decision-history row get appended. Legality of a particular
    /// transition (e.g. whether `Rejected` may return to `Proposed`) remains
    /// deliberately unenforced beyond that race -- ADR-0121 decision 3
    /// leaves the broader state-machine policy to a later decision.
    pub async fn record_decision(
        &mut self,
        request: RecordDecisionRequest,
    ) -> Result<(), DesignStoreError> {
        if request.actor.trim().is_empty() {
            return Err(DesignStoreError::EmptyActor);
        }
        let transaction = self.client.transaction().await?;
        let updated = transaction
            .execute(
                "UPDATE industrial_designs SET lifecycle_state = $4, updated_at = now() \
                 WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3 \
                   AND lifecycle_state = $5",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.design_id,
                    &(request.decision_kind as i16),
                    &(request.expected_lifecycle_state as i16),
                ],
            )
            .await?;
        if updated == 0 {
            let current = transaction
                .query_opt(
                    "SELECT lifecycle_state FROM industrial_designs \
                     WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3",
                    &[
                        &request.tenant_id,
                        &request.repository_id,
                        &request.design_id,
                    ],
                )
                .await?;
            transaction.commit().await?;
            return match current {
                None => Err(DesignStoreError::UnknownDesign {
                    design_id: request.design_id,
                }),
                Some(row) => {
                    let actual_value: i16 = row.get(0);
                    let actual = DesignLifecycleState::try_from(actual_value).map_err(|()| {
                        DesignStoreError::CorruptLifecycleState {
                            design_id: request.design_id.clone(),
                            value: actual_value,
                        }
                    })?;
                    Err(DesignStoreError::LifecycleStateConflict {
                        design_id: request.design_id,
                        expected: request.expected_lifecycle_state,
                        actual,
                    })
                }
            };
        }
        let next_sequence: i64 = transaction
            .query_one(
                "SELECT COALESCE(MAX(sequence_number), 0) + 1 FROM industrial_design_decisions \
                 WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.design_id,
                ],
            )
            .await?
            .get(0);
        transaction
            .execute(
                "INSERT INTO industrial_design_decisions \
                 (tenant_id, repository_id, design_id, sequence_number, decision_kind, actor, \
                  rationale) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.design_id,
                    &next_sequence,
                    &(request.decision_kind as i16),
                    &request.actor,
                    &request.rationale,
                ],
            )
            .await?;
        transaction.commit().await?;
        Ok(())
    }
}

mod listing;

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
        }
    }

    #[tokio::test]
    async fn a_created_design_reads_back_as_proposed() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let design_id = unique_id("design");
        let mut store = DesignStore::connect(&database_url).await.unwrap();

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
    async fn get_design_returns_none_for_an_unknown_design() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let store = DesignStore::connect(&database_url).await.unwrap();

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
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let design_id = unique_id("design");
        let mut store = DesignStore::connect(&database_url).await.unwrap();

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
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let design_id = unique_id("design");
        let mut store = DesignStore::connect(&database_url).await.unwrap();

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
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let design_id = unique_id("design");
        let mut store = DesignStore::connect(&database_url).await.unwrap();
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
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let mut store = DesignStore::connect(&database_url).await.unwrap();
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
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let design_id = unique_id("design");
        let mut store = DesignStore::connect(&database_url).await.unwrap();
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
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let mut store = DesignStore::connect(&database_url).await.unwrap();
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
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let mut store = DesignStore::connect(&database_url).await.unwrap();
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
        let mut work_store = crate::work_store::WorkStore::connect(&database_url)
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

        let mut store = DesignStore::connect(&database_url).await.unwrap();
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

//! Industrial design materialization revisions (ADR-0121 decision 4).
//!
//! A materialization revision records who materialized a design, against
//! which Constitution publication, referencing which Industrial Work tasks
//! and Lodestar goal ids, and why -- as an append-only, idempotency-key
//! scoped revision history. It is never a task-generation side effect: a
//! revision is recorded only when a caller explicitly submits one, and an
//! identical resubmission (same `idempotency_key`, same field values) is a
//! no-op returning the original revision, matching `evidence_store`'s own
//! established idempotency-key contract (field-by-field comparison, not a
//! raw digest compare -- the digest is a stored artifact for the caller,
//! not the store's own conflict check). Work-task references are a real
//! foreign key via a junction table, never a bare unchecked array.

use std::time::SystemTime;

use sha2::{Digest, Sha256};

use crate::db_pool::{PgConnection, PgPool};

const MIGRATION: &str =
    include_str!("../../migrations/0032_industrial_design_materializations.sql");

// Dependency migrations: 0032 foreign-keys into `industrial_designs`,
// `constitution_publications`, and `work_tasks`, none of them created by
// this store's own migration key; `industrial_designs` (0027) in turn
// foreign-keys into `evidence_records`. See design_store::mod's identical
// note -- a shared, long-lived local dev database hides this until tested
// against a genuinely fresh Postgres instance.
const EVIDENCE_DEPENDENCY_MIGRATION: &str = include_str!("../../migrations/0014_evidence.sql");
const INDUSTRIAL_DESIGNS_DEPENDENCY_MIGRATION: &str =
    include_str!("../../migrations/0027_industrial_designs.sql");
const CONSTITUTION_PUBLICATION_HISTORY_DEPENDENCY_MIGRATION: &str =
    include_str!("../../migrations/0026_constitution_publication_history.sql");
const WORK_DEPENDENCY_MIGRATION: &str = include_str!("../../migrations/0028_work.sql");

const MAX_ACTOR_BYTES: usize = 256;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;
const MAX_RATIONALE_BYTES: usize = 8192;
const MAX_GOAL_IDS: usize = 32;
const MAX_GOAL_ID_BYTES: usize = 256;
const MAX_WORK_TASK_IDS: usize = 32;

#[derive(Debug, thiserror::Error)]
pub enum MaterializationStoreError {
    #[error("materialization database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("materialization store could not obtain a database connection: {0}")]
    PoolExhausted(#[from] deadpool_postgres::PoolError),
    #[error("materialization actor must be between 1 and {MAX_ACTOR_BYTES} bytes")]
    InvalidActor,
    #[error(
        "materialization idempotency_key must be between 1 and {MAX_IDEMPOTENCY_KEY_BYTES} bytes"
    )]
    InvalidIdempotencyKey,
    #[error("materialization rationale must be at most {MAX_RATIONALE_BYTES} bytes")]
    InvalidRationale,
    #[error("materialization constitution_version_id must not be empty")]
    EmptyConstitutionVersionId,
    #[error("materialization may reference at most {MAX_GOAL_IDS} goal ids")]
    TooManyGoalIds,
    #[error("materialization goal ids must be between 1 and {MAX_GOAL_ID_BYTES} bytes each")]
    InvalidGoalId,
    #[error("materialization may reference at most {MAX_WORK_TASK_IDS} work tasks")]
    TooManyWorkTaskIds,
    #[error(
        "idempotency_key {idempotency_key} was already used for a design {design_id} materialization with different content"
    )]
    IdempotencyConflict {
        design_id: String,
        idempotency_key: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordMaterializationRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub design_id: String,
    pub actor: String,
    pub idempotency_key: String,
    pub rationale: Option<String>,
    pub constitution_version_id: String,
    pub work_task_ids: Vec<String>,
    pub goal_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MaterializationRevision {
    pub design_id: String,
    pub revision_number: i64,
    pub actor: String,
    pub idempotency_key: String,
    pub rationale: Option<String>,
    pub constitution_version_id: String,
    pub work_task_ids: Vec<String>,
    pub goal_ids: Vec<String>,
    pub payload_digest: Vec<u8>,
    pub recorded_at: SystemTime,
}

/// Private wire shape used only to compute `payload_digest` -- a
/// revision's queryable columns are the durable record.
#[derive(serde::Serialize)]
struct MaterializationPayload {
    actor: String,
    rationale: Option<String>,
    constitution_version_id: String,
    work_task_ids: Vec<String>,
    goal_ids: Vec<String>,
}

fn validate_request(
    request: &RecordMaterializationRequest,
) -> Result<(), MaterializationStoreError> {
    if request.actor.is_empty() || request.actor.len() > MAX_ACTOR_BYTES {
        return Err(MaterializationStoreError::InvalidActor);
    }
    if request.idempotency_key.is_empty()
        || request.idempotency_key.len() > MAX_IDEMPOTENCY_KEY_BYTES
    {
        return Err(MaterializationStoreError::InvalidIdempotencyKey);
    }
    if let Some(rationale) = &request.rationale {
        if rationale.len() > MAX_RATIONALE_BYTES {
            return Err(MaterializationStoreError::InvalidRationale);
        }
    }
    if request.constitution_version_id.trim().is_empty() {
        return Err(MaterializationStoreError::EmptyConstitutionVersionId);
    }
    if request.goal_ids.len() > MAX_GOAL_IDS {
        return Err(MaterializationStoreError::TooManyGoalIds);
    }
    if request
        .goal_ids
        .iter()
        .any(|id| id.is_empty() || id.len() > MAX_GOAL_ID_BYTES)
    {
        return Err(MaterializationStoreError::InvalidGoalId);
    }
    if request.work_task_ids.len() > MAX_WORK_TASK_IDS {
        return Err(MaterializationStoreError::TooManyWorkTaskIds);
    }
    Ok(())
}

pub struct MaterializationStore {
    pool: PgPool,
}

impl MaterializationStore {
    /// Takes a clone of the process's single pool (ADR-0143 decision 1), not
    /// a database URL: a store that resolved its own connection would be
    /// exactly the per-store demand the pool exists to bound.
    pub async fn connect(pool: &PgPool) -> Result<Self, MaterializationStoreError> {
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
            INDUSTRIAL_DESIGNS_DEPENDENCY_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut connection,
            crate::migration_lock::key::INDUSTRIAL_DESIGN_MATERIALIZATIONS,
            MIGRATION,
        )
        .await?;
        Ok(Self { pool: pool.clone() })
    }

    /// One checked-out connection, held only for the call that asked for it.
    ///
    /// A caller that opens a transaction keeps this binding alive for the
    /// life of that transaction, which is the one case where holding a
    /// connection across `.await` points is correct rather than accidental.
    async fn connection(&self) -> Result<PgConnection, MaterializationStoreError> {
        Ok(self.pool.get().await?)
    }

    /// Record one materialization revision. An identical resubmission (same
    /// `idempotency_key`, same every other field) returns the original
    /// revision unchanged; the same `idempotency_key` resubmitted with any
    /// different field is refused as a conflict, never silently overwritten
    /// or appended as a fresh revision.
    pub async fn record_materialization(
        &self,
        request: RecordMaterializationRequest,
    ) -> Result<MaterializationRevision, MaterializationStoreError> {
        validate_request(&request)?;

        if let Some(existing) = self
            .find_by_idempotency_key(
                &request.tenant_id,
                &request.repository_id,
                &request.design_id,
                &request.idempotency_key,
            )
            .await?
        {
            let matches = existing.actor == request.actor
                && existing.rationale == request.rationale
                && existing.constitution_version_id == request.constitution_version_id
                && existing.work_task_ids == request.work_task_ids
                && existing.goal_ids == request.goal_ids;
            if !matches {
                return Err(MaterializationStoreError::IdempotencyConflict {
                    design_id: request.design_id,
                    idempotency_key: request.idempotency_key,
                });
            }
            return Ok(existing);
        }

        let payload = MaterializationPayload {
            actor: request.actor.clone(),
            rationale: request.rationale.clone(),
            constitution_version_id: request.constitution_version_id.clone(),
            work_task_ids: request.work_task_ids.clone(),
            goal_ids: request.goal_ids.clone(),
        };
        let payload_bytes =
            serde_json::to_vec(&payload).expect("MaterializationPayload always serializes");
        let payload_digest = Sha256::digest(&payload_bytes).to_vec();

        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        let next_revision: i64 = transaction
            .query_one(
                "SELECT COALESCE(MAX(revision_number), 0) + 1 \
                 FROM industrial_design_materializations \
                 WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.design_id,
                ],
            )
            .await?
            .get(0);
        let recorded_at: SystemTime = transaction
            .query_one(
                "INSERT INTO industrial_design_materializations \
                 (tenant_id, repository_id, design_id, revision_number, actor, idempotency_key, \
                  rationale, constitution_version_id, goal_ids, payload_digest) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
                 RETURNING recorded_at",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.design_id,
                    &next_revision,
                    &request.actor,
                    &request.idempotency_key,
                    &request.rationale,
                    &request.constitution_version_id,
                    &request.goal_ids,
                    &payload_digest,
                ],
            )
            .await?
            .get(0);
        for work_task_id in &request.work_task_ids {
            transaction
                .execute(
                    "INSERT INTO industrial_design_materialization_work_tasks \
                     (tenant_id, repository_id, design_id, revision_number, work_task_id) \
                     VALUES ($1, $2, $3, $4, $5)",
                    &[
                        &request.tenant_id,
                        &request.repository_id,
                        &request.design_id,
                        &next_revision,
                        work_task_id,
                    ],
                )
                .await?;
        }
        transaction.commit().await?;

        Ok(MaterializationRevision {
            design_id: request.design_id,
            revision_number: next_revision,
            actor: request.actor,
            idempotency_key: request.idempotency_key,
            rationale: request.rationale,
            constitution_version_id: request.constitution_version_id,
            work_task_ids: request.work_task_ids,
            goal_ids: request.goal_ids,
            payload_digest,
            recorded_at,
        })
    }

    async fn find_by_idempotency_key(
        &self,
        tenant_id: &str,
        repository_id: &str,
        design_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<MaterializationRevision>, MaterializationStoreError> {
        let Some(row) = self
            .connection()
            .await?
            .query_opt(
                "SELECT revision_number, actor, rationale, constitution_version_id, goal_ids, \
                        payload_digest, recorded_at \
                 FROM industrial_design_materializations \
                 WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3 \
                   AND idempotency_key = $4",
                &[&tenant_id, &repository_id, &design_id, &idempotency_key],
            )
            .await?
        else {
            return Ok(None);
        };
        let revision_number: i64 = row.get(0);
        let work_task_ids = self
            .work_task_ids_for(tenant_id, repository_id, design_id, revision_number)
            .await?;
        Ok(Some(MaterializationRevision {
            design_id: design_id.to_string(),
            revision_number,
            actor: row.get(1),
            idempotency_key: idempotency_key.to_string(),
            rationale: row.get(2),
            constitution_version_id: row.get(3),
            work_task_ids,
            goal_ids: row.get(4),
            payload_digest: row.get(5),
            recorded_at: row.get(6),
        }))
    }

    async fn work_task_ids_for(
        &self,
        tenant_id: &str,
        repository_id: &str,
        design_id: &str,
        revision_number: i64,
    ) -> Result<Vec<String>, MaterializationStoreError> {
        let rows = self
            .connection()
            .await?
            .query(
                "SELECT work_task_id FROM industrial_design_materialization_work_tasks \
                 WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3 \
                   AND revision_number = $4 \
                 ORDER BY work_task_id",
                &[&tenant_id, &repository_id, &design_id, &revision_number],
            )
            .await?;
        Ok(rows.into_iter().map(|row| row.get(0)).collect())
    }
}

mod listing;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constitution_store::{
        ClauseSnapshot, ConstitutionStore, RecordConstitutionPublicationRequest,
    };
    use crate::design_store::{CreateDesignRequest, DesignStore};
    use crate::work_store::{NewWorkTask, WorkStore};

    fn unique_id(label: &str) -> String {
        format!("{label}-{}", crate::test_support::uuid_ish())
    }

    struct Fixture {
        tenant_id: String,
        repository_id: String,
        design_id: String,
        constitution_version_id: String,
    }

    async fn build_fixture(database_url: &str) -> Fixture {
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repository");
        let design_id = unique_id("design");
        let version_id = unique_id("version");

        let mut constitution_store = ConstitutionStore::connect(database_url).await.unwrap();
        constitution_store
            .record_publication(RecordConstitutionPublicationRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                version_id: version_id.clone(),
                schema_version: "v1".to_string(),
                status: "active".to_string(),
                clauses: vec![ClauseSnapshot {
                    id: "clause-a".to_string(),
                    slug: "clause-a-slug".to_string(),
                    kind: "constraint".to_string(),
                    title: "a clause".to_string(),
                    statement: "a statement".to_string(),
                    status: "active".to_string(),
                    consequence: None,
                    scope: None,
                    rationale: None,
                }],
                source_reference: None,
                source_digest: None,
                published_at: SystemTime::now(),
            })
            .await
            .expect("recording the fixture publication should succeed");

        let design_store = DesignStore::connect(
            &crate::db_pool::build_pool(database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
                .expect("the test database url should build a pool"),
        )
        .await
        .unwrap();
        design_store
            .create_design(CreateDesignRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                design_id: design_id.clone(),
                title: "a design".to_string(),
                summary: "a summary".to_string(),
                source_version: "v1".to_string(),
                constitution_version_id: None,
                work_task_id: None,
                evidence_id: None,
                proposed_by: "agent:test".to_string(),
            })
            .await
            .expect("creating the fixture design should succeed");

        Fixture {
            tenant_id,
            repository_id,
            design_id,
            constitution_version_id: version_id,
        }
    }

    fn request(fixture: &Fixture, idempotency_key: &str) -> RecordMaterializationRequest {
        RecordMaterializationRequest {
            tenant_id: fixture.tenant_id.clone(),
            repository_id: fixture.repository_id.clone(),
            design_id: fixture.design_id.clone(),
            actor: "agent:materializer".to_string(),
            idempotency_key: idempotency_key.to_string(),
            rationale: Some("because it was accepted".to_string()),
            constitution_version_id: fixture.constitution_version_id.clone(),
            work_task_ids: vec![],
            goal_ids: vec!["goal:example@constitution:v4".to_string()],
        }
    }

    #[tokio::test]
    async fn a_recorded_materialization_reads_back_as_revision_one() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let fixture = build_fixture(&database_url).await;
        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let store = MaterializationStore::connect(&pool).await.unwrap();

        let revision = store
            .record_materialization(request(&fixture, "key-1"))
            .await
            .expect("recording a materialization should succeed");

        assert_eq!(revision.revision_number, 1);
        assert_eq!(
            revision.goal_ids,
            vec!["goal:example@constitution:v4".to_string()]
        );

        let fetched = store
            .get_materialization(
                &fixture.tenant_id,
                &fixture.repository_id,
                &fixture.design_id,
                1,
            )
            .await
            .expect("reading the revision should succeed")
            .expect("the revision should exist");
        assert_eq!(fetched, revision);
    }

    #[tokio::test]
    async fn an_identical_resubmission_is_an_idempotent_no_op() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let fixture = build_fixture(&database_url).await;
        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let store = MaterializationStore::connect(&pool).await.unwrap();

        let first = store
            .record_materialization(request(&fixture, "key-1"))
            .await
            .expect("the first submission should succeed");
        let second = store
            .record_materialization(request(&fixture, "key-1"))
            .await
            .expect("an identical resubmission should be a no-op, not an error");

        assert_eq!(first.revision_number, second.revision_number);
        let revisions = store
            .list_materializations(
                &fixture.tenant_id,
                &fixture.repository_id,
                &fixture.design_id,
            )
            .await
            .expect("listing revisions should succeed");
        assert_eq!(
            revisions.len(),
            1,
            "the retry must not append a second revision"
        );
    }

    #[tokio::test]
    async fn the_same_idempotency_key_with_different_content_is_refused() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let fixture = build_fixture(&database_url).await;
        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let store = MaterializationStore::connect(&pool).await.unwrap();

        store
            .record_materialization(request(&fixture, "key-1"))
            .await
            .expect("the first submission should succeed");

        let mut different = request(&fixture, "key-1");
        different.rationale = Some("a completely different rationale".to_string());
        let result = store.record_materialization(different).await;

        assert!(matches!(
            result,
            Err(MaterializationStoreError::IdempotencyConflict { .. })
        ));
    }

    #[tokio::test]
    async fn distinct_submissions_get_increasing_revision_numbers() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let fixture = build_fixture(&database_url).await;
        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let store = MaterializationStore::connect(&pool).await.unwrap();

        let first = store
            .record_materialization(request(&fixture, "key-1"))
            .await
            .expect("the first submission should succeed");
        let second = store
            .record_materialization(request(&fixture, "key-2"))
            .await
            .expect("a second, distinct submission should succeed");

        assert_eq!(first.revision_number, 1);
        assert_eq!(second.revision_number, 2);
    }

    #[tokio::test]
    async fn a_reference_to_a_nonexistent_constitution_version_is_refused() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let fixture = build_fixture(&database_url).await;
        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let store = MaterializationStore::connect(&pool).await.unwrap();

        let mut invalid = request(&fixture, "key-1");
        invalid.constitution_version_id = unique_id("nonexistent-version");
        let result = store.record_materialization(invalid).await;

        assert!(matches!(
            result,
            Err(MaterializationStoreError::Database(_))
        ));
    }

    #[tokio::test]
    async fn a_reference_to_a_real_work_task_succeeds_and_reads_back() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let fixture = build_fixture(&database_url).await;
        let task_id = unique_id("task");
        let mut work_store = WorkStore::connect(&database_url).await.unwrap();
        work_store
            .create_task(
                &NewWorkTask {
                    tenant_id: fixture.tenant_id.clone(),
                    repository_id: fixture.repository_id.clone(),
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

        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let store = MaterializationStore::connect(&pool).await.unwrap();
        let mut with_task = request(&fixture, "key-1");
        with_task.work_task_ids = vec![task_id.clone()];
        let revision = store
            .record_materialization(with_task)
            .await
            .expect("a materialization referencing a real work task should succeed");

        assert_eq!(revision.work_task_ids, vec![task_id]);
    }

    #[tokio::test]
    async fn a_reference_to_a_nonexistent_work_task_is_refused() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let fixture = build_fixture(&database_url).await;
        let pool = crate::db_pool::build_pool(&database_url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let store = MaterializationStore::connect(&pool).await.unwrap();

        let mut invalid = request(&fixture, "key-1");
        invalid.work_task_ids = vec![unique_id("nonexistent-task")];
        let result = store.record_materialization(invalid).await;

        assert!(matches!(
            result,
            Err(MaterializationStoreError::Database(_))
        ));
    }
}

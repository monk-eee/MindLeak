//! ADR-0120: an Industrial repository's optional, Ackplane-authoritative Work
//! namespace -- a bounded task projection plus its append-only history and
//! typed-wait log, and Board Doctor: "a deterministic diagnostic projection
//! over Work, ClaimStore, and publication freshness" (decision 7).
//!
//! This first slice is deliberately narrower than the full ADR: it provides
//! task creation, a paged/filterable list, task detail, and Board Doctor
//! findings -- decision 7's "first Bridge Work surface is read-only and
//! bounded". It does not yet implement the full event-sourced
//! expected-prior-version/idempotency-key optimistic concurrency decision 3
//! describes. Authenticated NodeSync work creation supplies the initial
//! idempotent event in production; later lifecycle mutations remain deferred.
//! Bridge-exposed mutation commands remain out of scope entirely per decision
//! 8.
#![allow(dead_code)]

use std::time::SystemTime;

use crate::db_pool::{PgConnection, PgPool};
use thiserror::Error;

const WORK_MIGRATION: &str = include_str!("../../migrations/0028_work.sql");
const CLAIM_MIGRATION: &str = include_str!("../../migrations/0005_claim_delegation.sql");
const WORK_TASK_COMMAND_EXECUTION_MIGRATION: &str =
    include_str!("../../migrations/0039_work_task_command_execution.sql");
const WORK_EVENT_POSITIONS_MIGRATION: &str =
    include_str!("../../migrations/0065_work_event_positions.sql");

#[derive(Debug, Error)]
pub enum WorkStoreError {
    #[error("work store database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("work store could not obtain a database connection: {0}")]
    PoolExhausted(#[from] deadpool_postgres::PoolError),
    #[error("unknown work task state: {value}")]
    UnknownState { value: i16 },
    #[error("task {tenant_id}/{repository_id}/{task_id} already exists")]
    TaskConflict {
        tenant_id: String,
        repository_id: String,
        task_id: String,
    },
}

/// Read-write access to a tenant's Industrial Work namespace.
pub struct WorkStore {
    pool: PgPool,
}

impl WorkStore {
    /// Takes a clone of the process's single pool (ADR-0143 decision 1), not a
    /// database URL: a store that resolved its own connection would be exactly
    /// the per-store demand the pool exists to bound.
    pub async fn connect(pool: &PgPool) -> Result<Self, WorkStoreError> {
        let mut client = pool.get().await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::CLAIM_DELEGATION,
            CLAIM_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::WORK,
            WORK_MIGRATION,
        )
        .await?;
        // Every WorkStore query now materializes the versioned task projection.
        // Apply the same schema evolution as WorkCommandStore before a Bridge
        // read or Node creation reaches `work_tasks.version`.
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::WORK_TASK_COMMAND_EXECUTION,
            WORK_TASK_COMMAND_EXECUTION_MIGRATION,
        )
        .await?;
        // ADR-0120 decision 3's ordered event stream: allocate positions for
        // history and record on the projection which event built it.
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::WORK_EVENT_POSITIONS,
            WORK_EVENT_POSITIONS_MIGRATION,
        )
        .await?;
        Ok(Self { pool: pool.clone() })
    }

    /// One checked-out connection, held only for the call that asked for it.
    ///
    /// A caller that opens a transaction keeps this binding alive for the life
    /// of that transaction — which is load-bearing here: ADR-0120 decision 3
    /// requires the Work event and its projection update to land in one
    /// transaction, so both must run on the same connection.
    pub(in crate::work_store) async fn connection(&self) -> Result<PgConnection, WorkStoreError> {
        Ok(self.pool.get().await?)
    }

    /// Records the initial event and the current-state projection in one
    /// transaction (ADR-0120 decision 3).
    pub async fn create_task(
        &self,
        task: &NewWorkTask,
        event_id: &str,
        now: SystemTime,
    ) -> Result<WorkTask, WorkStoreError> {
        let digest = source_digest(task);
        // One connection, held until commit: ADR-0120 decision 3 requires the
        // event and its projection update to land in one transaction.
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;
        let stream_position =
            allocate_stream_position(&transaction, &task.tenant_id, &task.repository_id).await?;
        let inserted = transaction
            .execute(
                "INSERT INTO work_tasks (tenant_id, repository_id, task_id, title, acceptance, \
                    goal_id, state, declared_paths, declared_symbols, source_digest, \
                    published_by, version, source_event_position, created_at, updated_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,1,$12,$13,$13) \
                 ON CONFLICT (tenant_id, repository_id, task_id) DO NOTHING",
                &[
                    &task.tenant_id,
                    &task.repository_id,
                    &task.task_id,
                    &task.title,
                    &task.acceptance,
                    &task.goal_id,
                    &WorkTaskState::Open.as_i16(),
                    &task.declared_paths,
                    &task.declared_symbols,
                    &digest,
                    &task.published_by,
                    &stream_position,
                    &now,
                ],
            )
            .await?;
        if inserted == 0 {
            return Err(WorkStoreError::TaskConflict {
                tenant_id: task.tenant_id.clone(),
                repository_id: task.repository_id.clone(),
                task_id: task.task_id.clone(),
            });
        }
        transaction
            .execute(
                "INSERT INTO work_task_history (tenant_id, repository_id, event_id, task_id, \
                    event_kind, from_state, to_state, actor_id, source_digest, stream_position, \
                    recorded_at) \
                 VALUES ($1,$2,$3,$4,1,NULL,$5,$6,$7,$8,$9)",
                &[
                    &task.tenant_id,
                    &task.repository_id,
                    &event_id,
                    &task.task_id,
                    &WorkTaskState::Open.as_i16(),
                    &task.published_by,
                    &digest,
                    &stream_position,
                    &now,
                ],
            )
            .await?;
        transaction.commit().await?;
        Ok(WorkTask {
            tenant_id: task.tenant_id.clone(),
            repository_id: task.repository_id.clone(),
            task_id: task.task_id.clone(),
            title: task.title.clone(),
            acceptance: task.acceptance.clone(),
            goal_id: task.goal_id.clone(),
            state: WorkTaskState::Open,
            owner_id: None,
            owner_session_id: None,
            lease_expires_at: None,
            declared_paths: task.declared_paths.clone(),
            declared_symbols: task.declared_symbols.clone(),
            published_by: task.published_by.clone(),
            version: 1,
            source_event_position: Some(stream_position),
            route_reference: None,
            created_at: now,
            updated_at: now,
        })
    }

    pub(crate) fn row_to_task(row: &tokio_postgres::Row) -> Result<WorkTask, WorkStoreError> {
        Ok(WorkTask {
            tenant_id: row.get("tenant_id"),
            repository_id: row.get("repository_id"),
            task_id: row.get("task_id"),
            title: row.get("title"),
            acceptance: row.get("acceptance"),
            goal_id: row.get("goal_id"),
            state: WorkTaskState::from_i16(row.get("state"))?,
            owner_id: row.get("owner_id"),
            owner_session_id: row.get("owner_session_id"),
            lease_expires_at: row.get("lease_expires_at"),
            declared_paths: row.get("declared_paths"),
            declared_symbols: row.get("declared_symbols"),
            published_by: row.get("published_by"),
            version: row.get("version"),
            source_event_position: row.get("source_event_position"),
            route_reference: row.get("route_reference"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    /// One page of tasks, newest-updated first, optionally filtered to one
    /// state (ADR-0112 bounded pagination).
    pub async fn list_tasks(
        &self,
        tenant_id: &str,
        repository_id: &str,
        state: Option<WorkTaskState>,
        page: i64,
        page_size: i64,
    ) -> Result<WorkTaskPage, WorkStoreError> {
        let offset = (page - 1) * page_size;
        // One checkout for either branch: both arms are the same read with a
        // different filter, so taking two connections would be accidental.
        let connection = self.connection().await?;
        let rows = match state {
            Some(state) => {
                connection
                    .query(
                        "SELECT *, COUNT(*) OVER()::BIGINT AS total_count FROM work_tasks \
                         WHERE tenant_id = $1 AND repository_id = $2 AND state = $3 \
                         ORDER BY updated_at DESC, task_id ASC LIMIT $4 OFFSET $5",
                        &[
                            &tenant_id,
                            &repository_id,
                            &state.as_i16(),
                            &page_size,
                            &offset,
                        ],
                    )
                    .await?
            }
            None => {
                connection
                    .query(
                        "SELECT *, COUNT(*) OVER()::BIGINT AS total_count FROM work_tasks \
                         WHERE tenant_id = $1 AND repository_id = $2 \
                         ORDER BY updated_at DESC, task_id ASC LIMIT $3 OFFSET $4",
                        &[&tenant_id, &repository_id, &page_size, &offset],
                    )
                    .await?
            }
        };
        let total = rows.first().map(|row| row.get("total_count")).unwrap_or(0);
        let mut items = Vec::with_capacity(rows.len());
        for row in &rows {
            items.push(Self::row_to_task(row)?);
        }
        Ok(WorkTaskPage { items, total })
    }

    pub async fn task_detail(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
    ) -> Result<Option<WorkTaskDetail>, WorkStoreError> {
        let Some(task_row) = self
            .connection()
                .await?
            .query_opt(
                "SELECT * FROM work_tasks WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
                &[&tenant_id, &repository_id, &task_id],
            )
            .await?
        else {
            return Ok(None);
        };
        let task = Self::row_to_task(&task_row)?;
        let history_rows = self
            .connection()
            .await?
            .query(
                "SELECT event_id, from_state, to_state, actor_id, stream_position, recorded_at \
                 FROM work_task_history \
                 WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 \
                 ORDER BY stream_position DESC",
                &[&tenant_id, &repository_id, &task_id],
            )
            .await?;
        let mut history = Vec::with_capacity(history_rows.len());
        for row in &history_rows {
            let from_state: Option<i16> = row.get("from_state");
            history.push(WorkTaskEvent {
                event_id: row.get("event_id"),
                task_id: task_id.to_owned(),
                stream_position: row.get("stream_position"),
                from_state: from_state.map(WorkTaskState::from_i16).transpose()?,
                to_state: WorkTaskState::from_i16(row.get("to_state"))?,
                actor_id: row.get("actor_id"),
                recorded_at: row.get("recorded_at"),
            });
        }
        let wait_rows = self
            .connection()
            .await?
            .query(
                "SELECT wait_id, question, audience, asked_by, asked_at, answered_by, answer, \
                    answered_at FROM work_task_waits \
                 WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 \
                 ORDER BY asked_at DESC",
                &[&tenant_id, &repository_id, &task_id],
            )
            .await?;
        let waits = wait_rows
            .iter()
            .map(|row| WorkTaskWait {
                wait_id: row.get("wait_id"),
                task_id: task_id.to_owned(),
                question: row.get("question"),
                audience: row.get("audience"),
                asked_by: row.get("asked_by"),
                asked_at: row.get("asked_at"),
                answered_by: row.get("answered_by"),
                answer: row.get("answer"),
                answered_at: row.get("answered_at"),
            })
            .collect();
        Ok(Some(WorkTaskDetail {
            task,
            history,
            waits,
        }))
    }
}

/// Hands out the next slot in a repository's Work stream, inside the caller's
/// transaction (ADR-0120 decision 3).
///
/// The `ON CONFLICT DO UPDATE` takes a row lock on that repository's head for
/// the rest of the transaction, so two concurrent creators in the same
/// repository serialize here rather than both reading the same head and
/// claiming the same position. That is the whole point of an allocated
/// position: it is what makes a gap in the sequence mean "an event is
/// missing". Serializing per repository is also why the head is keyed by
/// `(tenant_id, repository_id)` and not globally — unrelated repositories
/// never wait on each other.
///
/// A caller that then fails rolls the allocation back with everything else, so
/// a refused creation leaves no hole.
///
/// Returns the raw driver error rather than a `WorkStoreError` so that
/// `work_command_store`, whose lifecycle events share this same stream, can use
/// it through its own error type without either module depending on the
/// other's.
pub(crate) async fn allocate_stream_position(
    transaction: &tokio_postgres::Transaction<'_>,
    tenant_id: &str,
    repository_id: &str,
) -> Result<i64, tokio_postgres::Error> {
    let row = transaction
        .query_one(
            "INSERT INTO work_stream_heads (tenant_id, repository_id, stream_position) \
             VALUES ($1,$2,1) \
             ON CONFLICT (tenant_id, repository_id) \
             DO UPDATE SET stream_position = work_stream_heads.stream_position + 1 \
             RETURNING stream_position",
            &[&tenant_id, &repository_id],
        )
        .await?;
    Ok(row.get("stream_position"))
}

mod doctor;
mod ingress;
mod model;
mod publication;
pub use doctor::{FleetUnansweredWait, WorkDoctorFinding};
pub(crate) use ingress::WorkTaskCreationOutcome;
pub(in crate::work_store) use model::source_digest;
pub use model::{
    NewWorkTask, WorkTask, WorkTaskDetail, WorkTaskEvent, WorkTaskPage, WorkTaskState, WorkTaskWait,
};
pub use publication::{ClaimsOnlyWork, WorkPublication};

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;
    use crate::test_support::unique_id;

    fn new_task(tenant_id: &str, repository_id: &str, task_id: &str, title: &str) -> NewWorkTask {
        NewWorkTask {
            tenant_id: tenant_id.to_owned(),
            repository_id: repository_id.to_owned(),
            task_id: task_id.to_owned(),
            title: title.to_owned(),
            acceptance: "acceptance text".to_owned(),
            goal_id: None,
            declared_paths: Vec::new(),
            declared_symbols: Vec::new(),
            published_by: "test-actor".to_owned(),
        }
    }

    #[tokio::test]
    async fn a_created_task_appears_in_the_list_as_open() {
        let Some(pool) = crate::test_support::test_pool() else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let task_id = unique_id("task");
        let store = WorkStore::connect(&pool).await.expect("connect");
        let now = SystemTime::now();
        store
            .create_task(
                &new_task(&tenant_id, &repository_id, &task_id, "Ship the thing"),
                &unique_id("event"),
                now,
            )
            .await
            .expect("create task");

        let page = store
            .list_tasks(&tenant_id, &repository_id, None, 1, 20)
            .await
            .expect("list tasks");

        assert_eq!(page.total, 1);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].task_id, task_id);
        assert_eq!(page.items[0].title, "Ship the thing");
        assert_eq!(page.items[0].state, WorkTaskState::Open);
        assert_eq!(page.items[0].owner_id, None);
        assert_eq!(
            page.items[0].version, 1,
            "WorkStore::connect must apply the version-column migration before creation"
        );
    }

    #[tokio::test]
    async fn creating_the_same_task_twice_is_a_conflict() {
        let Some(pool) = crate::test_support::test_pool() else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let task_id = unique_id("task");
        let store = WorkStore::connect(&pool).await.expect("connect");
        let task = new_task(&tenant_id, &repository_id, &task_id, "Ship the thing");
        store
            .create_task(&task, &unique_id("event"), SystemTime::now())
            .await
            .expect("create task once");

        let result = store
            .create_task(&task, &unique_id("event"), SystemTime::now())
            .await;

        assert!(matches!(result, Err(WorkStoreError::TaskConflict { .. })));
    }

    #[tokio::test]
    async fn task_detail_includes_the_creation_event() {
        let Some(pool) = crate::test_support::test_pool() else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let task_id = unique_id("task");
        let store = WorkStore::connect(&pool).await.expect("connect");
        let event_id = unique_id("event");
        store
            .create_task(
                &new_task(&tenant_id, &repository_id, &task_id, "Ship the thing"),
                &event_id,
                SystemTime::now(),
            )
            .await
            .expect("create task");

        let detail = store
            .task_detail(&tenant_id, &repository_id, &task_id)
            .await
            .expect("task detail")
            .expect("task exists");

        assert_eq!(detail.task.task_id, task_id);
        assert_eq!(detail.history.len(), 1);
        assert_eq!(detail.history[0].event_id, event_id);
        assert_eq!(detail.history[0].from_state, None);
        assert_eq!(detail.history[0].to_state, WorkTaskState::Open);
        assert!(detail.waits.is_empty());
    }

    #[tokio::test]
    async fn a_task_that_does_not_exist_has_no_detail() {
        let Some(pool) = crate::test_support::test_pool() else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let store = WorkStore::connect(&pool).await.expect("connect");

        let detail = store
            .task_detail(&unique_id("tenant"), &unique_id("repo"), &unique_id("task"))
            .await
            .expect("task detail query");

        assert_eq!(detail, None);
    }

    /// ADR-0120 decision 3: the stream is dense from 1 within a repository, and
    /// each task's projection points at the event that created it. Both halves
    /// matter — a position nothing records is not a position anyone can compare
    /// against, which is what left `lagging` unstateable.
    #[tokio::test]
    async fn positions_are_dense_per_repository_and_recorded_on_the_projection() {
        let Some(pool) = crate::test_support::test_pool() else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let store = WorkStore::connect(&pool).await.expect("connect");

        let mut created = Vec::new();
        for index in 0..3 {
            let task_id = unique_id("task");
            let task = store
                .create_task(
                    &new_task(&tenant_id, &repository_id, &task_id, "Ship the thing"),
                    &unique_id("event"),
                    SystemTime::now(),
                )
                .await
                .expect("create task");
            assert_eq!(
                task.source_event_position,
                Some(index + 1),
                "a fresh repository's stream starts at 1 and advances by one per event"
            );
            created.push(task);
        }

        for task in &created {
            let detail = store
                .task_detail(&tenant_id, &repository_id, &task.task_id)
                .await
                .expect("task detail")
                .expect("task exists");
            assert_eq!(
                detail.history[0].stream_position,
                task.source_event_position.unwrap(),
                "the projection must name the position of the event that built it"
            );
        }
    }

    /// The head is keyed by `(tenant_id, repository_id)`, so an unrelated
    /// repository never inherits another's position. A globally-keyed head
    /// would still look correct in a single-repository test.
    #[tokio::test]
    async fn each_repository_numbers_its_own_stream_from_one() {
        let Some(pool) = crate::test_support::test_pool() else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let first_repository = unique_id("repo");
        let second_repository = unique_id("repo");
        let store = WorkStore::connect(&pool).await.expect("connect");

        let first = store
            .create_task(
                &new_task(&tenant_id, &first_repository, &unique_id("task"), "First"),
                &unique_id("event"),
                SystemTime::now(),
            )
            .await
            .expect("create in first repository");
        let second = store
            .create_task(
                &new_task(&tenant_id, &second_repository, &unique_id("task"), "Second"),
                &unique_id("event"),
                SystemTime::now(),
            )
            .await
            .expect("create in second repository");

        assert_eq!(first.source_event_position, Some(1));
        assert_eq!(
            second.source_event_position,
            Some(1),
            "a second repository starts its own stream, it does not continue the first's"
        );
    }

    /// A refused creation must not burn a position. The allocation happens
    /// inside the same transaction as the insert that fails, so the rollback
    /// takes it with it; if it did not, the next successful event would leave a
    /// gap that reads as "an event is missing".
    #[tokio::test]
    async fn a_refused_creation_leaves_no_gap_in_the_stream() {
        let Some(pool) = crate::test_support::test_pool() else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let store = WorkStore::connect(&pool).await.expect("connect");
        let duplicate = new_task(&tenant_id, &repository_id, &unique_id("task"), "First");
        let first = store
            .create_task(&duplicate, &unique_id("event"), SystemTime::now())
            .await
            .expect("create task once");
        assert_eq!(first.source_event_position, Some(1));

        let refused = store
            .create_task(&duplicate, &unique_id("event"), SystemTime::now())
            .await;
        assert!(matches!(refused, Err(WorkStoreError::TaskConflict { .. })));

        let next = store
            .create_task(
                &new_task(&tenant_id, &repository_id, &unique_id("task"), "Second"),
                &unique_id("event"),
                SystemTime::now(),
            )
            .await
            .expect("create the next task");
        assert_eq!(
            next.source_event_position,
            Some(2),
            "the refused creation rolled its allocation back, so the stream stays dense"
        );
    }
}

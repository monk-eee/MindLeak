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

use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio_postgres::Client;

const WORK_MIGRATION: &str = include_str!("../../migrations/0028_work.sql");
const CLAIM_MIGRATION: &str = include_str!("../../migrations/0005_claim_delegation.sql");

/// ADR-0120 decision 3's eight lifecycle states, in the order the decision
/// text lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkTaskState {
    Open,
    Claimed,
    Waiting,
    Paused,
    Blocked,
    InReview,
    Completed,
    Abandoned,
}

impl WorkTaskState {
    fn as_i16(self) -> i16 {
        match self {
            Self::Open => 1,
            Self::Claimed => 2,
            Self::Waiting => 3,
            Self::Paused => 4,
            Self::Blocked => 5,
            Self::InReview => 6,
            Self::Completed => 7,
            Self::Abandoned => 8,
        }
    }

    fn from_i16(value: i16) -> Result<Self, WorkStoreError> {
        match value {
            1 => Ok(Self::Open),
            2 => Ok(Self::Claimed),
            3 => Ok(Self::Waiting),
            4 => Ok(Self::Paused),
            5 => Ok(Self::Blocked),
            6 => Ok(Self::InReview),
            7 => Ok(Self::Completed),
            8 => Ok(Self::Abandoned),
            other => Err(WorkStoreError::UnknownState { value: other }),
        }
    }

    /// `completed`/`abandoned`: a Board Doctor scope-overlap or duplicate-
    /// title finding never compares two tasks if either has left the board.
    fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Abandoned)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkTask {
    pub tenant_id: String,
    pub repository_id: String,
    pub task_id: String,
    pub title: String,
    pub acceptance: String,
    pub goal_id: Option<String>,
    pub state: WorkTaskState,
    pub owner_id: Option<String>,
    pub owner_session_id: Option<String>,
    pub lease_expires_at: Option<SystemTime>,
    pub declared_paths: Vec<String>,
    pub declared_symbols: Vec<String>,
    pub published_by: String,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// A new task's initial event (ADR-0120 decision 2). `source_digest` covers
/// its immutable bounded content; the event identity and publisher bind the
/// remaining replay authority.
#[derive(Debug, Clone, PartialEq)]
pub struct NewWorkTask {
    pub tenant_id: String,
    pub repository_id: String,
    pub task_id: String,
    pub title: String,
    pub acceptance: String,
    pub goal_id: Option<String>,
    pub declared_paths: Vec<String>,
    pub declared_symbols: Vec<String>,
    pub published_by: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkTaskPage {
    pub items: Vec<WorkTask>,
    pub total: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkTaskWait {
    pub wait_id: String,
    pub task_id: String,
    pub question: String,
    pub audience: Option<String>,
    pub asked_by: String,
    pub asked_at: SystemTime,
    pub answered_by: Option<String>,
    pub answer: Option<String>,
    pub answered_at: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkTaskEvent {
    pub event_id: String,
    pub task_id: String,
    pub from_state: Option<WorkTaskState>,
    pub to_state: WorkTaskState,
    pub actor_id: String,
    pub recorded_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WorkTaskDetail {
    pub task: WorkTask,
    pub history: Vec<WorkTaskEvent>,
    pub waits: Vec<WorkTaskWait>,
}

#[derive(Debug, Error)]
pub enum WorkStoreError {
    #[error("work store database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("unknown work task state: {value}")]
    UnknownState { value: i16 },
    #[error("task {tenant_id}/{repository_id}/{task_id} already exists")]
    TaskConflict {
        tenant_id: String,
        repository_id: String,
        task_id: String,
    },
}

fn source_digest(task: &NewWorkTask) -> Vec<u8> {
    let mut hasher = Sha256::new();
    append_digest_part(&mut hasher, b"mindleak.ackplane.work.task.v1");
    append_digest_part(&mut hasher, task.title.as_bytes());
    append_digest_part(&mut hasher, task.acceptance.as_bytes());
    match &task.goal_id {
        Some(goal_id) => {
            hasher.update([1]);
            append_digest_part(&mut hasher, goal_id.as_bytes());
        }
        None => hasher.update([0]),
    }
    for values in [&task.declared_paths, &task.declared_symbols] {
        hasher.update((values.len() as u64).to_be_bytes());
        for value in values {
            append_digest_part(&mut hasher, value.as_bytes());
        }
    }
    hasher.finalize().to_vec()
}

fn append_digest_part(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

/// Read-write access to a tenant's Industrial Work namespace.
pub struct WorkStore {
    client: Client,
}

impl WorkStore {
    pub async fn connect(database_url: &str) -> Result<Self, WorkStoreError> {
        let (mut client, connection) =
            tokio_postgres::connect(database_url, tokio_postgres::NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane work store connection closed with an error");
            }
        });
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
        Ok(Self { client })
    }

    /// Records the initial event and the current-state projection in one
    /// transaction (ADR-0120 decision 3).
    pub async fn create_task(
        &mut self,
        task: &NewWorkTask,
        event_id: &str,
        now: SystemTime,
    ) -> Result<WorkTask, WorkStoreError> {
        let digest = source_digest(task);
        let transaction = self.client.transaction().await?;
        let inserted = transaction
            .execute(
                "INSERT INTO work_tasks (tenant_id, repository_id, task_id, title, acceptance, \
                    goal_id, state, declared_paths, declared_symbols, source_digest, \
                    published_by, created_at, updated_at) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$12) \
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
                    event_kind, from_state, to_state, actor_id, source_digest, recorded_at) \
                 VALUES ($1,$2,$3,$4,1,NULL,$5,$6,$7,$8)",
                &[
                    &task.tenant_id,
                    &task.repository_id,
                    &event_id,
                    &task.task_id,
                    &WorkTaskState::Open.as_i16(),
                    &task.published_by,
                    &digest,
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
            created_at: now,
            updated_at: now,
        })
    }

    fn row_to_task(row: &tokio_postgres::Row) -> Result<WorkTask, WorkStoreError> {
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
        let rows = match state {
            Some(state) => {
                self.client
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
                self.client
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
            .client
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
            .client
            .query(
                "SELECT event_id, from_state, to_state, actor_id, recorded_at FROM work_task_history \
                 WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 \
                 ORDER BY recorded_at DESC, event_id DESC",
                &[&tenant_id, &repository_id, &task_id],
            )
            .await?;
        let mut history = Vec::with_capacity(history_rows.len());
        for row in &history_rows {
            let from_state: Option<i16> = row.get("from_state");
            history.push(WorkTaskEvent {
                event_id: row.get("event_id"),
                task_id: task_id.to_owned(),
                from_state: from_state.map(WorkTaskState::from_i16).transpose()?,
                to_state: WorkTaskState::from_i16(row.get("to_state"))?,
                actor_id: row.get("actor_id"),
                recorded_at: row.get("recorded_at"),
            });
        }
        let wait_rows = self
            .client
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

mod doctor;
mod ingress;
mod publication;

pub use doctor::WorkDoctorFinding;
pub(crate) use ingress::WorkTaskCreationOutcome;
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
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let task_id = unique_id("task");
        let mut store = WorkStore::connect(&database_url).await.expect("connect");
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
    }

    #[tokio::test]
    async fn creating_the_same_task_twice_is_a_conflict() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let task_id = unique_id("task");
        let mut store = WorkStore::connect(&database_url).await.expect("connect");
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
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let tenant_id = unique_id("tenant");
        let repository_id = unique_id("repo");
        let task_id = unique_id("task");
        let mut store = WorkStore::connect(&database_url).await.expect("connect");
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
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
            return;
        };
        let store = WorkStore::connect(&database_url).await.expect("connect");

        let detail = store
            .task_detail(&unique_id("tenant"), &unique_id("repo"), &unique_id("task"))
            .await
            .expect("task detail query");

        assert_eq!(detail, None);
    }
}

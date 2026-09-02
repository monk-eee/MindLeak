//! ADR-0120's Work value objects: the lifecycle state, the bounded task
//! projection and its history, and the digest that binds a new task's
//! immutable content.
//!
//! These carry no connection and run no query, so they stay separable from the
//! store that reads and writes them.

use std::time::SystemTime;

use sha2::{Digest, Sha256};

use super::WorkStoreError;

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
    pub(in crate::work_store) fn as_i16(self) -> i16 {
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

    pub(in crate::work_store) fn from_i16(value: i16) -> Result<Self, WorkStoreError> {
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
    pub(in crate::work_store) fn is_terminal(self) -> bool {
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
    /// Optimistic-concurrency version (ADR-0120 decision 3 / ADR-0125
    /// decision 5). Starts at 1 and increments once per applied Work-command
    /// effect; never decreases, never resets.
    pub version: i64,
    /// The position of the event this projection row was last built from
    /// (ADR-0120 decision 3). `None` means "never projected" — which is a
    /// different fact from position zero, the same distinction
    /// `0002_projection.sql` draws for the ledger projection.
    pub source_event_position: Option<i64>,
    /// The bounded route or assignment reference `RouteWork` last recorded
    /// (ADR-0125 decision 1). `None` until routed.
    pub route_reference: Option<String>,
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
    /// This event's slot in its repository's Work stream (ADR-0120 decision
    /// 3). Dense and gap-free from 1, so a reader can tell "I have every event
    /// up to here" from the positions alone — which `recorded_at` could never
    /// support, being a clock reading that ties.
    pub stream_position: i64,
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

pub(in crate::work_store) fn source_digest(task: &NewWorkTask) -> Vec<u8> {
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

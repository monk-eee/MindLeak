use serde::{Deserialize, Serialize};

use crate::{daemon::DaemonError, OutboxError};

mod cli;
mod confirm;
mod inspect;
pub(crate) mod record;

pub use cli::{execute, USAGE};
pub use confirm::confirm;
pub use inspect::inspect;

#[cfg(test)]
mod tests;

#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error("recovery refused: {0}")]
    Refused(String),
    #[error("recovery file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("recovery record error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("recovery evidence: {0}")]
    Outbox(#[from] OutboxError),
    #[error("recovery database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("recovery delivery: {0}")]
    Daemon(#[from] DaemonError),
    #[error("recovery node operation: {0}")]
    Node(#[from] Box<ackplane_client::ClientError>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryPreview {
    pub slot: String,
    pub run_id: String,
    pub identity: ackplane_protocol::supervisor::SupervisorIdentity,
    pub session_id: String,
    pub worker_id: String,
    pub task_id: String,
    pub working_directory: std::path::PathBuf,
    pub branch: String,
    pub marker_digest: String,
    pub confirmation_digest: String,
    pub stopped: bool,
    pub cleanup_recorded: bool,
    pub acknowledged: u64,
    pub last_enqueued: u64,
    pub pending_frames: usize,
}

pub use mindleak_model::{ModelCallProvenance, ModelCallSource, ModelFailure, ModelFailureReason};
use thiserror::Error;

/// Errors surfaced by the MindLeak core engine.
#[derive(Error, Debug)]
pub enum MindLeakError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ollama/http error: {0}")]
    Http(String),

    #[error(transparent)]
    Model(#[from] ModelFailure),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("busy: {0}")]
    Busy(String),

    #[error("cancelled: {0}")]
    Cancelled(String),

    #[error("{0}")]
    Other(String),
}

impl MindLeakError {
    pub fn model_failure(&self) -> Option<&ModelFailure> {
        match self {
            MindLeakError::Model(failure) => Some(failure),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, MindLeakError>;

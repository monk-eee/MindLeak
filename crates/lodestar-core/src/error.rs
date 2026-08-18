pub use mindleak_model::{ModelFailure, ModelFailureReason};
use thiserror::Error;

/// Errors surfaced by the Lodestar Intent Plane.
#[derive(Error, Debug)]
pub enum LodestarError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Model(#[from] ModelFailure),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid: {0}")]
    Invalid(String),

    #[error("federated claim source error: {0}")]
    Federated(String),
}

impl LodestarError {
    pub fn model_failure(&self) -> Option<&ModelFailure> {
        match self {
            LodestarError::Model(failure) => Some(failure),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, LodestarError>;

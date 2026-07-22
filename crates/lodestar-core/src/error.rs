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

    #[error("llm/http error: {0}")]
    Http(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, LodestarError>;

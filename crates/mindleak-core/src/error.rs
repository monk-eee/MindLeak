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

    #[error("not found: {0}")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, MindLeakError>;

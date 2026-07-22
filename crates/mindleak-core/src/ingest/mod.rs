//! Zero-token deterministic ingestion: turn raw telemetry into graph triples
//! using pattern matching only (no LLM tokens on the write path).

pub mod ast;
pub mod execution;
pub mod git;
pub(crate) mod javascript;
pub mod structure;

use sha2::{Digest, Sha256};

/// Short stable hash used to build deterministic node ids.
pub(crate) fn short_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().take(6).map(|b| format!("{b:02x}")).collect();
    hex
}

/// Normalise a filesystem path to forward slashes for stable node ids.
pub(crate) fn normalize_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// Truncate a string to `max` chars (char-safe), appending an ellipsis marker.
pub(crate) fn clamp(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push_str(" …");
    out
}

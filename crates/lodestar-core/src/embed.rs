//! Optional semantic index over learned knowledge (ADR-0080).
//!
//! Knowledge was searchable only by exact substring, so a lesson reached an
//! agent who already knew which file or goal to ask about, and nobody who was
//! asking a question. This module adds the missing half: an embedding per
//! statement, and cosine ranking behind the read surface agents already call.
//!
//! **Why this duplicates `mindleak_core::embed` rather than reusing it.**
//! `mindleak-core` is a *dev-dependency only* of `lodestar-core` (ADR-0004), so
//! the Intent Plane's runtime cannot reach the Memory Plane's embedder at all.
//! Reusing it would mean promoting that dependency and coupling the two planes,
//! which is the decoupling ADR-0004 exists to protect. Extracting a third,
//! shared crate is the other way to remove the duplication and was deliberately
//! not taken here: it buys one copy of ~60 lines at the cost of a new published
//! surface for both planes to version against. If a third consumer appears,
//! that trade changes and the extraction becomes worth doing.
//!
//! Everything here is optional. No embedder means no index, and every caller
//! degrades to substring matching rather than failing.

use std::time::Duration;

use rusqlite::{params, Connection};

use crate::error::{LodestarError, Result};
use crate::model::Knowledge;

/// A filtered exchange with an unreachable server must fail fast rather than
/// hang: this runs on the knowledge write path, where a stall is worse than
/// having no index at all.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(750);
const CALL_TIMEOUT: Duration = Duration::from_secs(15);

/// Active knowledge plus an honest statement of how it was matched (ADR-0080).
///
/// The mode travels with the results because a degraded answer that looks
/// identical to a good one teaches the reader to trust it equally.
#[derive(Debug, Clone)]
pub struct KnowledgeMatches {
    pub knowledge: Vec<Knowledge>,
    /// `"semantic"`, `"substring"`, or `"weight"` when nothing was asked.
    pub mode: &'static str,
    /// Why the requested mode was not available, when it was not.
    pub degraded_because: Option<String>,
}

/// A local, OpenAI-compatible embeddings client (points at Ollama by default).
#[derive(Debug, Clone)]
pub struct Embedder {
    pub base_url: String,
    pub model: String,
    api_key: String,
}

impl Default for Embedder {
    fn default() -> Self {
        Embedder {
            base_url: std::env::var("LODESTAR_EMBED_URL")
                .or_else(|_| std::env::var("MINDLEAK_EMBED_URL"))
                .unwrap_or_else(|_| "http://localhost:11434/v1".to_string()),
            model: std::env::var("LODESTAR_EMBED_MODEL")
                .or_else(|_| std::env::var("MINDLEAK_EMBED_MODEL"))
                .unwrap_or_else(|_| "nomic-embed-text".to_string()),
            api_key: std::env::var("LODESTAR_EMBED_API_KEY").unwrap_or_default(),
        }
    }
}

impl Embedder {
    /// Embed one statement, or error cleanly when no model is reachable.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/embeddings", self.base_url.trim_end_matches('/'));
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(CONNECT_TIMEOUT)
            .timeout(CALL_TIMEOUT)
            .build();
        let mut request = agent.post(&url).set("content-type", "application/json");
        if !self.api_key.is_empty() {
            request = request.set("authorization", &format!("Bearer {}", self.api_key));
        }
        let response = request
            .send_json(ureq::json!({ "model": self.model, "input": [text] }))
            .map_err(|e| LodestarError::Invalid(format!("embedder unreachable: {e}")))?;
        let body: serde_json::Value = response
            .into_json()
            .map_err(|e| LodestarError::Invalid(format!("embedder returned no JSON: {e}")))?;
        let vector: Vec<f32> = body["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| LodestarError::Invalid("embedder returned no embedding".into()))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();
        if vector.is_empty() {
            return Err(LodestarError::Invalid(
                "embedder returned an empty vector".into(),
            ));
        }
        Ok(vector)
    }
}

/// Cosine similarity. Zero for a degenerate vector, so an unembeddable
/// statement ranks last rather than dividing by zero.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let left: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let right: f32 = b.iter().map(|y| y * y).sum::<f32>().sqrt();
    if left == 0.0 || right == 0.0 {
        return 0.0;
    }
    dot / (left * right)
}

/// This module owns its table, so adding the index needs no schema migration.
pub(crate) fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS knowledge_embeddings (
             knowledge_id TEXT NOT NULL,
             model        TEXT NOT NULL,
             dim          INTEGER NOT NULL,
             vector       BLOB NOT NULL,
             updated_at   INTEGER NOT NULL,
             PRIMARY KEY (knowledge_id, model)
         );",
        [],
    )?;
    Ok(())
}

pub(crate) fn store_vector(
    conn: &Connection,
    knowledge_id: &str,
    model: &str,
    vector: &[f32],
    now: i64,
) -> Result<()> {
    ensure_table(conn)?;
    let bytes: Vec<u8> = vector.iter().flat_map(|f| f.to_le_bytes()).collect();
    conn.execute(
        "INSERT INTO knowledge_embeddings (knowledge_id, model, dim, vector, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(knowledge_id, model) DO UPDATE SET
             dim = excluded.dim, vector = excluded.vector, updated_at = excluded.updated_at",
        params![knowledge_id, model, vector.len() as i64, bytes, now],
    )?;
    Ok(())
}

/// Whether this statement still needs embedding.
///
/// A knowledge id is the hash of its statement, so re-recording the same lesson
/// would otherwise re-embed identical text on every confirmation — a network
/// call per write, for a vector we already hold.
pub(crate) fn needs_vector(conn: &Connection, knowledge_id: &str, model: &str) -> Result<bool> {
    ensure_table(conn)?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM knowledge_embeddings WHERE knowledge_id = ?1 AND model = ?2",
        params![knowledge_id, model],
        |row| row.get(0),
    )?;
    Ok(count == 0)
}

/// Every stored vector for `model`, as (knowledge id, vector).
pub(crate) fn vectors_for_model(conn: &Connection, model: &str) -> Result<Vec<(String, Vec<f32>)>> {
    ensure_table(conn)?;
    let mut stmt =
        conn.prepare("SELECT knowledge_id, vector FROM knowledge_embeddings WHERE model = ?1")?;
    let rows = stmt.query_map(params![model], |row| {
        let id: String = row.get(0)?;
        let bytes: Vec<u8> = row.get(1)?;
        Ok((id, decode_vector(&bytes)))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn decode_vector(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_is_one_for_identical_zero_for_orthogonal_and_degenerate() {
        assert!((cosine(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        // A zero vector has no direction, so it cannot be similar to anything.
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
        // Mismatched dimensions are a different model's output, never a match.
        assert_eq!(cosine(&[1.0, 2.0], &[1.0, 2.0, 3.0]), 0.0);
    }

    #[test]
    fn a_stored_vector_round_trips_through_its_blob() {
        let conn = Connection::open_in_memory().unwrap();
        store_vector(
            &conn,
            "knowledge:abc",
            "test-model",
            &[0.5, -0.25, 2.0],
            100,
        )
        .unwrap();

        let stored = vectors_for_model(&conn, "test-model").unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].0, "knowledge:abc");
        assert_eq!(stored[0].1, vec![0.5, -0.25, 2.0]);
    }

    #[test]
    fn a_statement_is_embedded_once_per_model_not_once_per_write() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(needs_vector(&conn, "knowledge:abc", "test-model").unwrap());

        store_vector(&conn, "knowledge:abc", "test-model", &[1.0], 100).unwrap();

        assert!(!needs_vector(&conn, "knowledge:abc", "test-model").unwrap());
        // A different model has its own vector space and its own index.
        assert!(needs_vector(&conn, "knowledge:abc", "other-model").unwrap());
    }

    #[test]
    fn re_embedding_replaces_rather_than_duplicates() {
        let conn = Connection::open_in_memory().unwrap();
        store_vector(&conn, "knowledge:abc", "test-model", &[1.0], 100).unwrap();
        store_vector(&conn, "knowledge:abc", "test-model", &[2.0], 200).unwrap();

        let stored = vectors_for_model(&conn, "test-model").unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].1, vec![2.0]);
    }
}

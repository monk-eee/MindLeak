//! Optional semantic-recall embedding index (ADR-0008).
//!
//! A *complement* to the decay graph, never a replacement (ADR-0002 stands). It
//! is **off the zero-token write path**: embeddings are produced by an explicit,
//! optional `index` pass over a local OpenAI-compatible `/v1/embeddings` server
//! (Ollama, LM Studio, …), exactly like consolidation. Recall returns
//! semantically-similar node ids to seed into graph traversal; the graph still
//! does the reasoning. The module owns its own `embeddings` table so it adds no
//! schema/migration coupling.

use rusqlite::{params, Connection};
use serde_json::json;

use crate::error::{MindLeakError, Result};

/// A local, OpenAI-compatible embeddings client (points at Ollama by default).
#[derive(Debug, Clone)]
pub struct Embedder {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
}

impl Default for Embedder {
    fn default() -> Self {
        Embedder {
            base_url: std::env::var("MINDLEAK_EMBED_URL")
                .unwrap_or_else(|_| "http://localhost:11434/v1".to_string()),
            model: std::env::var("MINDLEAK_EMBED_MODEL")
                .unwrap_or_else(|_| "nomic-embed-text".to_string()),
            api_key: std::env::var("MINDLEAK_EMBED_API_KEY").unwrap_or_default(),
        }
    }
}

impl Embedder {
    /// Embed `text` into a dense vector. Errors cleanly when no model is
    /// reachable — the feature is optional and never on the hot path.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let url = format!("{}/embeddings", self.base_url.trim_end_matches('/'));
        let mut req = ureq::post(&url).set("Content-Type", "application/json");
        if !self.api_key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", self.api_key));
        }
        let resp = req
            .send_json(json!({ "model": self.model, "input": text }))
            .map_err(|e| MindLeakError::Http(e.to_string()))?;
        let value: serde_json::Value = resp
            .into_json()
            .map_err(|e| MindLeakError::Http(e.to_string()))?;
        let embedding = value
            .get("data")
            .and_then(|d| d.get(0))
            .and_then(|d| d.get("embedding"))
            .and_then(|e| e.as_array())
            .ok_or_else(|| {
                MindLeakError::Http("embeddings response missing data[0].embedding".into())
            })?;
        let vector: Vec<f32> = embedding
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();
        if vector.is_empty() {
            return Err(MindLeakError::Http("empty embedding vector".into()));
        }
        Ok(vector)
    }
}

/// Cosine similarity of two equal-length vectors; 0.0 for degenerate input.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

fn to_blob(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn from_blob(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Create the embeddings table if it does not exist (idempotent, self-owned).
pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS embeddings (
             node_id    TEXT NOT NULL,
             model      TEXT NOT NULL,
             dim        INTEGER NOT NULL,
             vector     BLOB NOT NULL,
             updated_at INTEGER NOT NULL,
             PRIMARY KEY (node_id, model)
         );",
    )?;
    Ok(())
}

/// Insert or replace the embedding for `node_id` under `model`.
pub fn upsert(
    conn: &Connection,
    node_id: &str,
    model: &str,
    vector: &[f32],
    now: i64,
) -> Result<()> {
    ensure_table(conn)?;
    conn.execute(
        "INSERT INTO embeddings (node_id, model, dim, vector, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(node_id, model) DO UPDATE SET
             dim = excluded.dim,
             vector = excluded.vector,
             updated_at = excluded.updated_at",
        params![node_id, model, vector.len() as i64, to_blob(vector), now],
    )?;
    Ok(())
}

/// Rank stored nodes for `model` by cosine similarity to `query`, best first.
pub fn recall(
    conn: &Connection,
    query: &[f32],
    model: &str,
    limit: usize,
) -> Result<Vec<(String, f32)>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare("SELECT node_id, vector FROM embeddings WHERE model = ?1")?;
    let rows = stmt.query_map(params![model], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))
    })?;
    let mut scored = Vec::new();
    for row in rows {
        let (id, blob) = row?;
        scored.push((id, cosine(query, &from_blob(&blob))));
    }
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    Ok(scored)
}

/// Nodes that have no embedding for `model` yet, with the text to embed
/// (`label` + `content`). Used by the offline `index` pass.
pub fn nodes_missing_embeddings(
    conn: &Connection,
    model: &str,
    limit: usize,
) -> Result<Vec<(String, String)>> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, label, content FROM nodes
         WHERE id NOT IN (SELECT node_id FROM embeddings WHERE model = ?1)
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![model, limit as i64], |row| {
        let id: String = row.get(0)?;
        let label: String = row.get(1)?;
        let content: Option<String> = row.get(2)?;
        let text = match content {
            Some(c) => format!("{label}\n{c}"),
            None => label,
        };
        Ok((id, text))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    // Generated by AI (UnitTest MCP)
    use super::*;
    use crate::db;

    #[test]
    fn cosine_identical_orthogonal_and_degenerate() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[1.0, 0.0], &[1.0]), 0.0); // length mismatch
    }

    #[test]
    fn recall_ranks_by_cosine_similarity() {
        let conn = db::open_in_memory().unwrap();
        upsert(&conn, "artifact:a", "m", &[1.0, 0.0, 0.0], 1).unwrap();
        upsert(&conn, "artifact:b", "m", &[0.0, 1.0, 0.0], 1).unwrap();
        upsert(&conn, "artifact:c", "m", &[0.9, 0.1, 0.0], 1).unwrap();
        let hits = recall(&conn, &[1.0, 0.0, 0.0], "m", 2).unwrap();
        assert_eq!(hits[0].0, "artifact:a");
        assert_eq!(hits[1].0, "artifact:c"); // closer than b
    }

    #[test]
    fn upsert_replaces_vector_for_same_node_and_model() {
        let conn = db::open_in_memory().unwrap();
        upsert(&conn, "n", "m", &[1.0, 0.0], 1).unwrap();
        upsert(&conn, "n", "m", &[0.0, 1.0], 2).unwrap();
        let hits = recall(&conn, &[0.0, 1.0], "m", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert!((hits[0].1 - 1.0).abs() < 1e-6);
    }

    /// Live round-trip against a running embedding model (e.g.
    /// `ollama pull nomic-embed-text`). Ignored by default; run with
    /// `cargo test -p mindleak-core -- --ignored`.
    #[test]
    #[ignore = "requires a running local embedding model (e.g. nomic-embed-text)"]
    fn live_embed_round_trip() {
        let vector = Embedder::default()
            .embed("session token validation and jwt expiry")
            .expect("live embedding should return a vector");
        assert!(!vector.is_empty());
        eprintln!("live embed -> {} dims", vector.len());
    }
}

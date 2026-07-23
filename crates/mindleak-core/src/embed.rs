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
    /// reachable — the feature is optional and never on the hot path. Delegates
    /// to [`embed_batch`](Self::embed_batch) so single and batched paths share
    /// one implementation.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        self.embed_batch(std::slice::from_ref(&text.to_string()))?
            .into_iter()
            .next()
            .ok_or_else(|| MindLeakError::Http("empty embedding response".into()))
    }

    /// Embed many texts in a **single** request. The OpenAI-compatible
    /// `/v1/embeddings` endpoint accepts an array `input` and returns `data[]`
    /// (each carrying its `index`), so the offline `index` pass costs one round
    /// trip per batch instead of one per node. Vectors are returned in input
    /// order. Errors cleanly when no model is reachable — the error is
    /// *actionable*: it names the model, the URL, and the exact remediation so a
    /// missing embedding model can never be a silent 404 (ADR-0008).
    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}/embeddings", self.base_url.trim_end_matches('/'));
        let body = json!({ "model": self.model, "input": texts });
        let value = crate::net::post_json(
            &crate::net::HttpConfig::for_model(),
            &url,
            &self.api_key,
            &body,
        )
        .map_err(|error| {
            MindLeakError::Http(format!(
                "semantic recall is unavailable: embedding model '{model}' could not be reached \
                 at {url} ({error}). recall/index are optional (ADR-0008) and require a local \
                 OpenAI-compatible embeddings server. If you use Ollama, run \
                 `ollama pull {model}`; otherwise set MINDLEAK_EMBED_MODEL / MINDLEAK_EMBED_URL \
                 to a reachable model, or ignore this if you are not using semantic recall.",
                model = self.model,
            ))
        })?;
        let data = value
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| MindLeakError::Http("embeddings response missing data[]".into()))?;
        if data.len() != texts.len() {
            return Err(MindLeakError::Http(format!(
                "embeddings returned {} vectors for {} inputs",
                data.len(),
                texts.len()
            )));
        }
        let mut out: Vec<Vec<f32>> = vec![Vec::new(); texts.len()];
        for (position, item) in data.iter().enumerate() {
            let index = item
                .get("index")
                .and_then(|i| i.as_u64())
                .map(|i| i as usize)
                .unwrap_or(position);
            if index >= out.len() {
                return Err(MindLeakError::Http(
                    "embeddings response index out of range".into(),
                ));
            }
            let vector: Vec<f32> = item
                .get("embedding")
                .and_then(|e| e.as_array())
                .ok_or_else(|| {
                    MindLeakError::Http("embeddings response item missing embedding".into())
                })?
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            if vector.is_empty() {
                return Err(MindLeakError::Http("empty embedding vector".into()));
            }
            out[index] = vector;
        }
        if out.iter().any(Vec::is_empty) {
            return Err(MindLeakError::Http(
                "embeddings response was missing a vector".into(),
            ));
        }
        Ok(out)
    }
}

/// The embedding backend behind semantic recall (ADR-0008). A trait so the
/// index→recall→seed path can be exercised with a deterministic or unreachable
/// embedder in tests, never depending on a live model. The concrete
/// [`Embedder`] talks to a local `/v1/embeddings` server; callers inject an
/// alternative via `MindLeak::with_embedder`.
///
/// `Send + Sync` is required because `MindLeak` owns a boxed `TextEmbedder` and
/// the async maintenance worker moves the whole `MindLeak` into a background
/// thread (see the `Send` assertion in `lib.rs`).
pub trait TextEmbedder: Send + Sync {
    /// The model tag under which vectors are stored and queried.
    fn model(&self) -> &str;
    /// Embed `text` into a dense vector, erroring cleanly when unavailable.
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    /// Embed many texts, returning vectors in input order. The default fans out
    /// to [`embed`](Self::embed) so simple/test embedders work unchanged; the
    /// network [`Embedder`] overrides it to embed a whole batch in one request.
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|text| self.embed(text)).collect()
    }
}

impl TextEmbedder for Embedder {
    fn model(&self) -> &str {
        &self.model
    }
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        Embedder::embed(self, text)
    }
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Embedder::embed_batch(self, texts)
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

    // A `TextEmbedder` that only implements the single-text `embed` still gets a
    // working `embed_batch` via the default fan-out, preserving input order — so
    // the batched `index` pass works with any embedder, live or stubbed.
    #[test]
    fn default_embed_batch_fans_out_in_input_order() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Counting {
            calls: AtomicUsize,
        }
        impl TextEmbedder for Counting {
            fn model(&self) -> &str {
                "stub"
            }
            fn embed(&self, text: &str) -> Result<Vec<f32>> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(vec![text.len() as f32])
            }
        }

        let embedder = Counting {
            calls: AtomicUsize::new(0),
        };
        let out = embedder
            .embed_batch(&["a".to_string(), "bb".to_string(), "ccc".to_string()])
            .unwrap();
        assert_eq!(out, vec![vec![1.0], vec![2.0], vec![3.0]]);
        assert_eq!(embedder.calls.load(Ordering::SeqCst), 3);
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

    /// Regression: a missing/unreachable embedding model must produce an
    /// *actionable* error (naming the model, `ollama pull`, and the env
    /// overrides), never a bare mysterious HTTP status. Uses an unreachable URL
    /// so no live model is required.
    #[test]
    fn embed_error_is_actionable_when_model_unreachable() {
        let embedder = Embedder {
            base_url: "http://127.0.0.1:1/v1".to_string(),
            model: "nomic-embed-text".to_string(),
            api_key: String::new(),
        };
        let message = embedder.embed("hello").unwrap_err().to_string();
        assert!(
            message.contains("nomic-embed-text"),
            "error must name the model: {message}"
        );
        assert!(
            message.contains("ollama pull"),
            "error must name the remediation: {message}"
        );
        assert!(
            message.contains("MINDLEAK_EMBED_MODEL"),
            "error must name the config override: {message}"
        );
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

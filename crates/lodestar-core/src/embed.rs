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
    /// How many of `knowledge` were actually ranked by meaning. Anything beyond
    /// this count is unindexed and sits in weight order at the end — reported,
    /// because a partly-ranked list is otherwise indistinguishable from a
    /// fully-ranked one.
    pub ranked: usize,
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
        let mut vectors = self.embed_batch(std::slice::from_ref(&text.to_string()))?;
        Ok(vectors.remove(0))
    }

    /// Embed many statements in one request, returning vectors in input order.
    ///
    /// Backfilling an existing corpus one statement at a time is a round trip
    /// per lesson; batching makes warming the whole index a single call.
    pub fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
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
            .send_json(ureq::json!({ "model": self.model, "input": texts }))
            .map_err(|e| {
                LodestarError::Invalid(format!(
                    "embedding model '{model}' could not be reached at {url} ({e}). Knowledge \
                     search is optional and falls back to substring matching; to enable it, run \
                     `ollama pull {model}` or point LODESTAR_EMBED_URL / LODESTAR_EMBED_MODEL at a \
                     reachable OpenAI-compatible embeddings server.",
                    model = self.model,
                ))
            })?;
        let body: serde_json::Value = response
            .into_json()
            .map_err(|e| LodestarError::Invalid(format!("embedder returned no JSON: {e}")))?;
        parse_embedding_response(&body, texts.len())
    }
}

/// Parse an OpenAI-compatible embeddings response strictly.
///
/// Every check here exists because the lenient version of it fails silently: a
/// dropped component shortens a vector, a reordered `data[]` attaches a vector
/// to the wrong statement, and a mismatched dimension scores zero against
/// everything. All three produce confident, wrong rankings rather than an error.
fn parse_embedding_response(value: &serde_json::Value, expected: usize) -> Result<Vec<Vec<f32>>> {
    let data = value
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| LodestarError::Invalid("embeddings response missing data[]".into()))?;
    if data.len() != expected {
        return Err(LodestarError::Invalid(format!(
            "embeddings returned {} vectors for {expected} inputs",
            data.len()
        )));
    }
    let mut out: Vec<Vec<f32>> = vec![Vec::new(); expected];
    let mut dimension: Option<usize> = None;
    for (position, item) in data.iter().enumerate() {
        // `index` is authoritative when present: the API does not promise to
        // return vectors in the order they were submitted.
        let index = item
            .get("index")
            .and_then(|i| i.as_u64())
            .map_or(position, |i| i as usize);
        if index >= out.len() {
            return Err(LodestarError::Invalid(
                "embeddings response index out of range".into(),
            ));
        }
        let vector = parse_embedding_vector(item)?;
        match dimension {
            Some(expected_dim) if vector.len() != expected_dim => {
                return Err(LodestarError::Invalid(format!(
                    "embeddings response has inconsistent dimensions: expected {expected_dim}, \
                     got {}",
                    vector.len()
                )));
            }
            None => dimension = Some(vector.len()),
            Some(_) => {}
        }
        out[index] = vector;
    }
    if out.iter().any(Vec::is_empty) {
        return Err(LodestarError::Invalid(
            "embeddings response was missing a vector".into(),
        ));
    }
    Ok(out)
}

fn parse_embedding_vector(item: &serde_json::Value) -> Result<Vec<f32>> {
    let components = item
        .get("embedding")
        .and_then(|e| e.as_array())
        .ok_or_else(|| {
            LodestarError::Invalid("embeddings response item missing embedding".into())
        })?;
    if components.is_empty() {
        return Err(LodestarError::Invalid("empty embedding vector".into()));
    }
    components
        .iter()
        .enumerate()
        .map(|(position, value)| {
            let number = value.as_f64().ok_or_else(|| {
                LodestarError::Invalid(format!("embedding component {position} is not numeric"))
            })?;
            let narrowed = number as f32;
            if !narrowed.is_finite() {
                return Err(LodestarError::Invalid(format!(
                    "embedding component {position} is not finite as f32"
                )));
            }
            Ok(narrowed)
        })
        .collect()
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

/// Which of `ids` have no vector under `model` yet.
///
/// The index only ever filled on write, so a repository that learned anything
/// before it existed had a permanently empty index and searched by substring
/// forever. This is what lets a read warm it.
pub(crate) fn missing_vectors(
    conn: &Connection,
    model: &str,
    ids: &[String],
) -> Result<Vec<String>> {
    ensure_table(conn)?;
    let mut stmt =
        conn.prepare("SELECT 1 FROM knowledge_embeddings WHERE knowledge_id = ?1 AND model = ?2")?;
    let mut missing = Vec::new();
    for id in ids {
        if !stmt.exists(params![id, model])? {
            missing.push(id.clone());
        }
    }
    Ok(missing)
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

    /// A one-shot HTTP server returning `body`, so the request and response
    /// parsing are exercised for real rather than assumed.
    fn stub_embedder(body: &'static str) -> Embedder {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::{Read, Write};
                let _ = stream.read(&mut [0u8; 8192]);
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                         content-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
            }
        });
        Embedder {
            base_url: format!("http://{addr}/v1"),
            model: "stub".to_string(),
            api_key: String::new(),
        }
    }

    #[test]
    fn a_batch_round_trip_returns_one_vector_per_input_in_input_order() {
        // `index` deliberately arrives out of order: the API does not promise
        // response order, and attaching a vector to the wrong statement is a
        // wrong answer no later check can catch.
        let embedder = stub_embedder(
            r#"{"data":[{"index":1,"embedding":[0.0,1.0]},{"index":0,"embedding":[1.0,0.0]}]}"#,
        );

        let vectors = embedder
            .embed_batch(&["first".to_string(), "second".to_string()])
            .unwrap();

        assert_eq!(vectors, vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
    }

    #[test]
    fn an_empty_batch_asks_the_model_nothing() {
        // No server is listening on this port, so reaching the network at all
        // would fail the test rather than quietly cost a round trip.
        let embedder = Embedder {
            base_url: "http://127.0.0.1:9".to_string(),
            model: "stub".to_string(),
            api_key: String::new(),
        };
        assert!(embedder.embed_batch(&[]).unwrap().is_empty());
    }

    #[test]
    fn a_malformed_response_is_refused_rather_than_quietly_reshaped() {
        // Each of these once produced a confident, wrong ranking instead of an
        // error: a dropped component shortens a vector, a short data[] silently
        // drops a statement, and a mixed dimension scores zero against
        // everything it is compared with.
        let cases = [
            r#"{"data":[{"index":0,"embedding":[1.0,null]}]}"#,
            r#"{"data":[]}"#,
            r#"{"data":[{"index":0,"embedding":[1.0]},{"index":1,"embedding":[1.0,2.0]}]}"#,
            r#"{"data":[{"index":9,"embedding":[1.0]}]}"#,
            r#"{"data":[{"index":0,"embedding":[]}]}"#,
        ];
        let inputs = [
            vec!["a".to_string()],
            vec!["a".to_string()],
            vec!["a".to_string(), "b".to_string()],
            vec!["a".to_string()],
            vec!["a".to_string()],
        ];

        for (body, input) in cases.iter().zip(inputs) {
            assert!(
                stub_embedder(body).embed_batch(&input).is_err(),
                "should have refused: {body}"
            );
        }
    }

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

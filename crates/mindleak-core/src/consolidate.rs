//! Optional asynchronous LLM consolidation via a local, OpenAI-compatible model
//! server (Ollama's `/v1`, LM Studio, llama.cpp's server, …).
//!
//! The deterministic pipeline handles the write path with zero tokens. This
//! worker is the "sleep-phase" consolidation layer: it compresses batches of raw
//! logs into a single high-level `intent` node using a small local model
//! (e.g. `glm4:9b` / `codegeex4:9b`) and never leaves the machine.

use serde::{Deserialize, Serialize};

use crate::error::{MindLeakError, Result};
use crate::graph::{GraphStore, WriteOutcome};
use crate::ingest::{normalize_path, short_hash};
use crate::model::{Edge, Node, NodeType, RelationType};

/// Structured summary the model is asked to return.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentSummary {
    pub intent_label: String,
    #[serde(default)]
    pub impacted_files: Vec<String>,
    #[serde(default)]
    pub status: String,
}

/// An OpenAI-compatible consolidation client (points at a local server by default).
#[derive(Debug, Clone)]
pub struct Consolidator {
    /// OpenAI-compatible base URL, e.g. `http://localhost:11434/v1` (Ollama).
    pub base_url: String,
    pub model: String,
    /// Optional bearer token for hosted servers; empty means no auth header.
    pub api_key: String,
}

impl Default for Consolidator {
    fn default() -> Self {
        let base_url = std::env::var("MINDLEAK_LLM_URL")
            .unwrap_or_else(|_| "http://localhost:11434/v1".to_string());
        let model = std::env::var("MINDLEAK_MODEL").unwrap_or_else(|_| "glm4:9b".to_string());
        let api_key = std::env::var("MINDLEAK_LLM_API_KEY").unwrap_or_default();
        Consolidator {
            base_url,
            model,
            api_key,
        }
    }
}

impl Consolidator {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Consolidator {
            base_url: base_url.into(),
            model: model.into(),
            api_key: String::new(),
        }
    }

    /// Call the OpenAI-compatible `/chat/completions` endpoint with a JSON
    /// `response_format` and parse the returned summary.
    pub fn consolidate(&self, logs: &[String]) -> Result<IntentSummary> {
        let body = serde_json::json!({
            "model": self.model,
            "stream": false,
            "response_format": { "type": "json_object" },
            "messages": [
                {
                    "role": "system",
                    "content": "You are a background graph extraction engine. Compress raw execution logs into a single structured intent node. Respond with ONLY a JSON object with keys: intent_label (string), impacted_files (array of strings), status (string)."
                },
                { "role": "user", "content": logs.join("\n") }
            ]
        });

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut req = ureq::post(&url).set("Content-Type", "application/json");
        if !self.api_key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", self.api_key));
        }
        let resp = req
            .send_json(body)
            .map_err(|e| MindLeakError::Http(e.to_string()))?;

        let value: serde_json::Value = resp
            .into_json()
            .map_err(|e| MindLeakError::Http(e.to_string()))?;
        let content = value
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                MindLeakError::Http("chat completion missing choices[0].message.content".into())
            })?;

        let summary: IntentSummary = serde_json::from_str(extract_json(content))?;
        Ok(summary)
    }

    /// Consolidate a batch of logs and write the resulting intent into the graph,
    /// linked to each impacted artifact. Returns the created intent id + counts.
    pub fn consolidate_and_store(
        &self,
        store: &GraphStore,
        logs: &[String],
        now: i64,
    ) -> Result<(String, WriteOutcome)> {
        let summary = self.consolidate(logs)?;
        let mut outcome = WriteOutcome::default();

        let intent_id = format!("intent:{}", short_hash(&summary.intent_label));
        let intent = Node::new(
            &intent_id,
            NodeType::Intent,
            summary.intent_label.clone(),
            now,
        )
        .with_content(format!("status={}\n{}", summary.status, logs.join("\n")));
        if store.upsert_node(&intent)? {
            outcome.nodes_created += 1;
        }
        outcome.node_ids.push(intent_id.clone());

        for file in &summary.impacted_files {
            let path = normalize_path(file);
            let art_id = format!("artifact:{path}");
            let art = Node::new(&art_id, NodeType::Artifact, path.clone(), now);
            if store.upsert_node(&art)? {
                outcome.nodes_created += 1;
            }
            let edge = Edge::new(&intent_id, &art_id, RelationType::Refactored, now);
            if store.upsert_edge(&edge)? {
                outcome.edges_created += 1;
            }
        }

        Ok((intent_id, outcome))
    }
}

/// Extract the first JSON object from model content that may be wrapped in a
/// markdown code fence or surrounded by prose (glm4 and others do this even with
/// `response_format: json_object`). Braces are ASCII, so byte slicing is safe.
fn extract_json(content: &str) -> &str {
    match (content.find('{'), content.rfind('}')) {
        (Some(start), Some(end)) if end >= start => &content[start..=end],
        _ => content.trim(),
    }
}

#[cfg(test)]
mod tests {
    // Generated by AI (UnitTest MCP)
    use super::*;

    #[test]
    fn extract_json_pulls_object_from_fence_or_prose() {
        assert_eq!(extract_json("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(extract_json("Sure:\n{\"a\":1} done"), "{\"a\":1}");
        assert_eq!(extract_json("{\"a\":1}"), "{\"a\":1}");
    }

    #[test]
    fn new_sets_fields() {
        let c = Consolidator::new("http://host:1234", "codegeex4:9b");
        assert_eq!(c.base_url, "http://host:1234");
        assert_eq!(c.model, "codegeex4:9b");
    }

    #[test]
    fn intent_summary_defaults_missing_fields() {
        let s: IntentSummary =
            serde_json::from_str(r#"{"intent_label":"fix null token"}"#).unwrap();
        assert_eq!(s.intent_label, "fix null token");
        assert!(s.impacted_files.is_empty());
        assert_eq!(s.status, "");
    }

    #[test]
    fn intent_summary_parses_full_payload() {
        let json = r#"{"intent_label":"refactor auth","impacted_files":["src/auth.rs"],"status":"PASSING"}"#;
        let s: IntentSummary = serde_json::from_str(json).unwrap();
        assert_eq!(s.impacted_files, vec!["src/auth.rs"]);
        assert_eq!(s.status, "PASSING");
    }

    /// Live round-trip against a running OpenAI-compatible server (Ollama +
    /// `glm4:9b` by default). Ignored in normal runs; exercise with
    /// `cargo test -p mindleak-core -- --ignored`. Proves the model honours our
    /// JSON `response_format` contract end to end.
    #[test]
    #[ignore = "requires a running local model (e.g. `ollama run glm4:9b`)"]
    fn live_consolidate_round_trip() {
        let logs = vec![
            "$ cargo test  ->  exit 101".to_string(),
            "thread 'auth' panicked at src/auth.rs:42: token was null".to_string(),
            "$ cargo test  ->  exit 0 (after null-guarding the JWT path)".to_string(),
        ];
        let summary = Consolidator::default()
            .consolidate(&logs)
            .expect("live consolidation should return a parsed IntentSummary");
        assert!(
            !summary.intent_label.trim().is_empty(),
            "model returned an empty intent_label: {summary:?}"
        );
        eprintln!("live consolidate -> {summary:?}");
    }
}

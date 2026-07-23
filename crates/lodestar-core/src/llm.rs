//! Optional local, OpenAI-compatible client for goal decomposition and semantic
//! conformance. Same posture as MindLeak's consolidation worker: off every hot
//! path, and it degrades cleanly (callers fall back) when no server is reachable.

use serde_json::json;

use crate::error::{LodestarError, Result};

/// A candidate task produced by decomposition.
#[derive(Debug, Clone)]
pub struct TaskDraft {
    pub title: String,
    pub acceptance: String,
}

/// An OpenAI-compatible chat client pointed at a local server by default.
#[derive(Debug, Clone)]
pub struct LlmClient {
    pub base_url: String,
    pub model: String,
    pub api_key: String,
}

impl Default for LlmClient {
    fn default() -> Self {
        LlmClient {
            base_url: std::env::var("LODESTAR_LLM_URL")
                .unwrap_or_else(|_| "http://localhost:11434/v1".to_string()),
            model: std::env::var("LODESTAR_MODEL").unwrap_or_else(|_| "glm4:9b".to_string()),
            api_key: std::env::var("LODESTAR_LLM_API_KEY").unwrap_or_default(),
        }
    }
}

impl LlmClient {
    /// A client pointed at an unroutable endpoint (`127.0.0.1:1`). Every request
    /// fails fast, forcing model-optional callers (`decompose`, `judge`) down
    /// their deterministic fallback. Use this to keep tests independent of any
    /// ambient local model instead of hand-rolling an unreachable URL per call
    /// site.
    pub fn unreachable() -> Self {
        LlmClient {
            base_url: "http://127.0.0.1:1/v1".to_string(),
            model: "unreachable".to_string(),
            api_key: String::new(),
        }
    }

    /// POST `/chat/completions` with a strict JSON response and parse the content.
    fn chat_json(&self, system: &str, user: &str) -> Result<serde_json::Value> {
        let body = json!({
            "model": self.model,
            "stream": false,
            "response_format": { "type": "json_object" },
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ]
        });
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let timeout = std::time::Duration::from_millis(
            std::env::var("MINDLEAK_HTTP_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30_000),
        );
        let agent = ureq::builder()
            .timeout_connect(timeout)
            .timeout_read(timeout)
            .build();
        let mut req = agent.post(&url);
        if !self.api_key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", self.api_key));
        }
        let resp = req
            .send_json(body)
            .map_err(|e| LodestarError::Http(e.to_string()))?;
        let value: serde_json::Value = resp
            .into_json()
            .map_err(|e| LodestarError::Http(e.to_string()))?;
        let content = value
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| LodestarError::Http("missing choices[0].message.content".into()))?;
        Ok(serde_json::from_str(extract_json(content))?)
    }

    /// Break a goal into concrete, independently-claimable tasks.
    pub fn decompose(&self, goal_title: &str, goal_statement: &str) -> Result<Vec<TaskDraft>> {
        let system = "You are a planning engine. Break a goal into 2-6 concrete, \
             independently-claimable tasks. Respond with ONLY JSON: \
             {\"tasks\":[{\"title\":string,\"acceptance\":string}]}.";
        let user = format!("Goal: {goal_title}\n\n{goal_statement}");
        let value = self.chat_json(system, &user)?;
        let mut out = Vec::new();
        if let Some(arr) = value.get("tasks").and_then(|t| t.as_array()) {
            for t in arr {
                let title = t
                    .get("title")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if title.is_empty() {
                    continue;
                }
                let acceptance = t
                    .get("acceptance")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                out.push(TaskDraft { title, acceptance });
            }
        }
        Ok(out)
    }

    /// Judge whether a change violates a constraint. Returns `(verdict, rationale)`
    /// where verdict is one of `aligned` / `violation` / `needs_human`.
    pub fn judge(&self, constraint: &str, change_summary: &str) -> Result<(String, String)> {
        let system = "You check whether a change violates a constraint. Respond with \
             ONLY JSON: {\"verdict\":\"aligned\"|\"violation\"|\"needs_human\",\
             \"rationale\":string}.";
        let user = format!("Constraint: {constraint}\n\nChange touches: {change_summary}");
        let value = self.chat_json(system, &user)?;
        let verdict = value
            .get("verdict")
            .and_then(|x| x.as_str())
            .unwrap_or("needs_human")
            .to_string();
        let rationale = value
            .get("rationale")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        Ok((verdict, rationale))
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

    /// Live round-trip against a running OpenAI-compatible server (Ollama +
    /// `glm4:9b` by default). Ignored in normal runs; exercise with
    /// `cargo test -p lodestar-core -- --ignored`. Proves decomposition and the
    /// semantic conformance judge against the JSON contract end to end.
    #[test]
    #[ignore = "requires a running local model (e.g. `ollama run glm4:9b`)"]
    fn live_decompose_and_judge_round_trip() {
        let client = LlmClient::default();

        let tasks = client
            .decompose(
                "Add FTS search",
                "Expose full-text search over node labels and content via an MCP tool.",
            )
            .expect("live decompose should parse tasks");
        assert!(!tasks.is_empty(), "model returned no tasks");
        eprintln!("live decompose -> {} tasks", tasks.len());

        let (verdict, rationale) = client
            .judge(
                "The write path must never call an LLM (zero-token ingestion).",
                "crates/mindleak-core/src/ingest/ast.rs",
            )
            .expect("live judge should return a verdict");
        assert!(
            matches!(verdict.as_str(), "aligned" | "violation" | "needs_human"),
            "unexpected verdict: {verdict} ({rationale})"
        );
        eprintln!("live judge -> {verdict}: {rationale}");
    }
}

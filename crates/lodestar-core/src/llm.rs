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
        let mut req = ureq::post(&url).set("Content-Type", "application/json");
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
        Ok(serde_json::from_str(content)?)
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

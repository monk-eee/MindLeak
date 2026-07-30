//! Consolidated learned knowledge, and the promotion of a repeated signal
//! into something durable.

use serde::{Deserialize, Serialize};

/// A learned-knowledge row: a consolidated regularity with provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Knowledge {
    pub id: String,
    pub statement: String,
    pub evidence: String,
    pub weight: f64,
    pub half_life_hours: f64,
    pub confirmed_at: i64,
    pub created_at: i64,
}

impl Knowledge {
    /// The MindLeak node ids this knowledge was consolidated from, parsed
    /// best-effort from the stored `evidence` JSON (`{"nodes": [...]}`). Empty
    /// when the evidence is hand-authored or not in that shape — so a hand-written
    /// note never accidentally governs conformance.
    pub fn referenced_nodes(&self) -> Vec<String> {
        serde_json::from_str::<serde_json::Value>(&self.evidence)
            .ok()
            .and_then(|value| {
                value
                    .get("nodes")
                    .and_then(|nodes| nodes.as_array())
                    .map(|nodes| {
                        nodes
                            .iter()
                            .filter_map(|node| node.as_str().map(str::to_string))
                            .collect()
                    })
            })
            .unwrap_or_default()
    }

    /// The goal this lesson declares it was learned under, if the evidence says
    /// so directly (`{"goal": "goal:..."}`).
    ///
    /// A lesson that names no code nodes can never match the node-keyed
    /// advisory, but it is not therefore anonymous: most such records still
    /// carry the intent they were learned serving. Measured on this
    /// repository's ledger, 55 of 67 node-less records name a goal here or a
    /// task from which one is reachable.
    pub fn declared_goal(&self) -> Option<String> {
        serde_json::from_str::<serde_json::Value>(&self.evidence)
            .ok()
            .and_then(|value| {
                value
                    .get("goal")
                    .and_then(|goal| goal.as_str())
                    .map(str::to_string)
            })
    }

    /// The task ids named anywhere in the evidence, in order of appearance.
    ///
    /// Scanned as text rather than parsed, deliberately: this provenance was
    /// written by many hands over time and appears as a JSON field, inside
    /// nested arrays, and as the bare string `task:{id}` that is not JSON at
    /// all. A reader that only understood one shape would silence the records
    /// written in the others.
    pub fn referenced_tasks(&self) -> Vec<String> {
        let bytes = self.evidence.as_bytes();
        let mut found = Vec::new();
        let mut at = 0;
        while let Some(offset) = self.evidence[at..].find("task:") {
            let start = at + offset;
            let mut end = start + "task:".len();
            // A task id is hex; stop at the first character that cannot be one.
            while end < bytes.len() && bytes[end].is_ascii_hexdigit() {
                end += 1;
            }
            let id = &self.evidence[start..end];
            // `task:task:abc123` appears in the wild; skip the empty outer one.
            if end > start + "task:".len() && !found.contains(&id.to_string()) {
                found.push(id.to_string());
            }
            at = end.max(start + "task:".len());
        }
        found
    }
}

/// An opaque proven-signal candidate handed across the loose MindLeak → Lodestar
/// seam for gated promotion (ADR-0022). `evidence_node_ids` are MindLeak node ids
/// treated as opaque strings; the span comes from edge provenance. `statement`,
/// when present, is a pre-distilled summary (e.g. from a local model); when
/// absent the promoter builds a deterministic templated statement, so promotion
/// never depends on an LLM.
#[derive(Debug, Clone)]
pub struct SignalPromotion {
    pub subject: String,
    pub evidence_node_ids: Vec<String>,
    pub first_seen: i64,
    pub last_seen: i64,
    pub statement: Option<String>,
}

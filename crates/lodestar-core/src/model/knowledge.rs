//! Consolidated learned knowledge, and the proven signals promoted into it.

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

//! Ingest a git commit as an `intent` node linked to the artifacts it touched.

use crate::error::Result;
use crate::graph::{GraphStore, WriteOutcome};
use crate::ingest::{clamp, normalize_path, short_hash};
use crate::model::{Edge, Node, NodeType, RelationType};

/// A commit captured from git telemetry.
#[derive(Debug, Clone)]
pub struct CommitRecord {
    pub sha: Option<String>,
    pub message: String,
    pub changed_files: Vec<String>,
    pub timestamp: i64,
}

/// Extract any explicit decision/rationale markers from a commit message or
/// inline comment (`DECISION:`, `HACK:`, `WHY:`, `NOTE:`).
pub fn extract_rationale(message: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in message.lines() {
        let trimmed = line
            .trim()
            .trim_start_matches("//")
            .trim_start_matches('#')
            .trim();
        for marker in ["DECISION:", "HACK:", "WHY:", "NOTE:", "FIXME:"] {
            if let Some(idx) = trimmed.to_ascii_uppercase().find(marker) {
                let rest = trimmed[idx + marker.len()..].trim();
                if !rest.is_empty() {
                    out.push(format!("{marker} {rest}"));
                }
            }
        }
    }
    out
}

/// Ingest one commit. Creates an intent node and `refactored` edges to files.
pub fn ingest_commit(store: &GraphStore, rec: &CommitRecord, now: i64) -> Result<WriteOutcome> {
    let mut outcome = WriteOutcome::default();

    let key = rec
        .sha
        .clone()
        .unwrap_or_else(|| short_hash(&format!("{}|{}", rec.message, rec.timestamp)));
    let intent_id = format!("intent:{key}");
    let first_line = rec.message.lines().next().unwrap_or("").trim();
    let label = clamp(first_line, 80);

    let mut content = rec.message.clone();
    let rationale = extract_rationale(&rec.message);
    if !rationale.is_empty() {
        content.push_str("\n---\n");
        content.push_str(&rationale.join("\n"));
    }

    let intent =
        Node::new(&intent_id, NodeType::Intent, label, rec.timestamp).with_content(content);
    if store.upsert_node(&intent)? {
        outcome.nodes_created += 1;
    }
    outcome.node_ids.push(intent_id.clone());

    for file in &rec.changed_files {
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

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pulls_decision_markers() {
        let msg = "Fix login\n\n// DECISION: use SQLite over Neo4j for footprint";
        let r = extract_rationale(msg);
        assert!(r.iter().any(|l| l.contains("SQLite over Neo4j")));
    }
}

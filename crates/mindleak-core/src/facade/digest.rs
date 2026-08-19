use serde_json::json;

use crate::{
    now_unix, Digest, DigestSourceSnapshot, DigestStatus, DigestStatusReport, MindLeak,
    MindLeakError, Node, NodeType, Result,
};

/// `digest_type`s MindLeak ships as generic templates useful to any MCP
/// agent (ADR-0101 decision 6). A consuming product supplies its own
/// digest_type and template against this same compiler; it never gets a
/// private copy of the compiler.
const REPOSITORY_GUIDE: &str = "repository_guide";

impl MindLeak {
    /// Compile one named, typed digest from current graph state through a
    /// deterministic template (ADR-0101) — never hand-authored prose. Each
    /// call produces a new digest node rather than editing a prior one: to
    /// change a digest, change what it is compiled from and recompile.
    pub fn compile_digest(&self, digest_type: &str, limit: usize) -> Result<Digest> {
        match digest_type {
            REPOSITORY_GUIDE => self.compile_repository_guide(limit),
            other => Err(MindLeakError::Other(format!(
                "unsupported digest_type '{other}'; MindLeak ships: {REPOSITORY_GUIDE}"
            ))),
        }
    }

    /// A repository-guide digest: agent roster, recent decisions, and
    /// recently active files, composed entirely from existing tools
    /// (`counts`, `list_agents`, `snapshot`) — no new query, only a
    /// deterministic rendering over data those tools already return.
    fn compile_repository_guide(&self, limit: usize) -> Result<Digest> {
        let now = now_unix();
        let (node_count, edge_count) = self.counts()?;
        let agents = self.list_agents()?;
        let recent = self.snapshot(None, limit)?;

        let decisions: Vec<_> = recent
            .nodes
            .iter()
            .filter(|n| n.node.node_type == NodeType::Intent)
            .collect();
        let files: Vec<_> = recent
            .nodes
            .iter()
            .filter(|n| n.node.node_type == NodeType::Artifact)
            .collect();

        let mut markdown = String::new();
        markdown.push_str("# Repository guide\n\n");
        markdown.push_str(&format!(
            "Compiled from {node_count} nodes, {edge_count} active edges.\n\n"
        ));
        markdown.push_str("## Active agents\n\n");
        if agents.is_empty() {
            markdown.push_str("No attributed agent activity yet.\n\n");
        } else {
            for agent in &agents {
                markdown.push_str(&format!(
                    "- `{}` — {} observations, last active {}\n",
                    agent.label, agent.observations, agent.last_active
                ));
            }
            markdown.push('\n');
        }
        markdown.push_str("## Recent decisions\n\n");
        if decisions.is_empty() {
            markdown.push_str("No recorded decisions yet.\n\n");
        } else {
            for d in &decisions {
                markdown.push_str(&format!("- `{}` — {}\n", d.node.id, d.node.label));
            }
            markdown.push('\n');
        }
        markdown.push_str("## Recently active files\n\n");
        if files.is_empty() {
            markdown.push_str("No recently active files.\n\n");
        } else {
            for f in &files {
                markdown.push_str(&format!("- `{}`\n", f.node.id));
            }
            markdown.push('\n');
        }

        let source_node_ids: Vec<String> = recent
            .nodes
            .iter()
            .map(|n| n.node.id.clone())
            .chain(agents.iter().map(|a| a.id.clone()))
            .collect();

        let digest_id = format!("digest:{REPOSITORY_GUIDE}:{now}");
        let payload = json!({
            "digest_type": REPOSITORY_GUIDE,
            "template_id": REPOSITORY_GUIDE,
            "generated_at": now,
            "source_snapshot": { "node_ids": source_node_ids },
            "markdown": markdown,
        });
        let node = Node::new(&digest_id, NodeType::Digest, "repository guide", now)
            .with_content(payload.to_string());
        self.store.upsert_node(&node)?;

        Ok(Digest {
            id: digest_id,
            digest_type: REPOSITORY_GUIDE.to_string(),
            template_id: REPOSITORY_GUIDE.to_string(),
            generated_at: now,
            source_snapshot: DigestSourceSnapshot {
                node_ids: source_node_ids,
            },
            markdown,
        })
    }

    /// Whether a compiled digest's source snapshot still matches live graph
    /// state (ADR-0101 decision 4): `current` while every node it read from
    /// still exists, `stale` once any of them has been forgotten or reaped.
    /// Never regenerates on its own — a stale digest stays exactly as
    /// compiled until something explicitly recompiles it.
    pub fn digest_status(&self, digest_id: &str) -> Result<DigestStatusReport> {
        let node = self
            .store
            .get_node(digest_id)?
            .ok_or_else(|| MindLeakError::Other(format!("no digest found: {digest_id}")))?;
        if node.node_type != NodeType::Digest {
            return Err(MindLeakError::Other(format!(
                "{digest_id} is a {}, not a digest",
                node.node_type.as_str()
            )));
        }
        let content = node.content.clone().unwrap_or_default();
        let payload: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
            MindLeakError::Other(format!("digest {digest_id} content is not valid JSON: {e}"))
        })?;
        let generated_at = payload["generated_at"].as_i64().unwrap_or(node.created_at);
        let node_ids: Vec<String> = payload["source_snapshot"]["node_ids"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let mut missing_node_ids = Vec::new();
        for id in &node_ids {
            if self.store.get_node(id)?.is_none() {
                missing_node_ids.push(id.clone());
            }
        }
        let status = if missing_node_ids.is_empty() {
            DigestStatus::Current
        } else {
            DigestStatus::Stale
        };

        Ok(DigestStatusReport {
            digest_id: digest_id.to_string(),
            status,
            generated_at,
            missing_node_ids,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_a_repository_guide_from_agents_decisions_and_files() {
        let engine = MindLeak::open_in_memory().unwrap();
        engine
            .ingest_file_for_agent("agent-a", "src/a.rs", "fn a() {}\n")
            .unwrap();
        engine.record_decision("adopted X over Y", &[]).unwrap();

        let digest = engine.compile_digest("repository_guide", 10).unwrap();

        assert!(digest.id.starts_with("digest:repository_guide:"));
        assert!(digest.markdown.contains("# Repository guide"));
        assert!(digest.markdown.contains("agent-a"));
        assert!(!digest.source_snapshot.node_ids.is_empty());
    }

    #[test]
    fn rejects_an_unknown_digest_type() {
        let engine = MindLeak::open_in_memory().unwrap();
        let error = engine.compile_digest("weekly_report", 10).unwrap_err();
        assert!(error.to_string().contains("unsupported digest_type"));
    }

    #[test]
    fn digest_status_reports_current_immediately_after_compiling() {
        let engine = MindLeak::open_in_memory().unwrap();
        engine
            .ingest_file_for_agent("agent-a", "src/a.rs", "fn a() {}\n")
            .unwrap();
        let digest = engine.compile_digest("repository_guide", 10).unwrap();

        let status = engine.digest_status(&digest.id).unwrap();

        assert_eq!(status.status, DigestStatus::Current);
        assert!(status.missing_node_ids.is_empty());
    }

    #[test]
    fn digest_status_reports_stale_once_a_source_node_is_forgotten() {
        let engine = MindLeak::open_in_memory().unwrap();
        engine
            .ingest_file_for_agent("agent-a", "src/a.rs", "fn a() {}\n")
            .unwrap();
        let digest = engine.compile_digest("repository_guide", 10).unwrap();

        engine.forget_file("src/a.rs").unwrap();

        let status = engine.digest_status(&digest.id).unwrap();

        assert_eq!(status.status, DigestStatus::Stale);
        assert!(!status.missing_node_ids.is_empty());
    }

    #[test]
    fn digest_status_errors_for_a_node_that_is_not_a_digest() {
        let engine = MindLeak::open_in_memory().unwrap();
        engine.ingest_file("src/a.rs", "fn a() {}\n").unwrap();

        let error = engine.digest_status("artifact:src/a.rs").unwrap_err();

        assert!(error.to_string().contains("not a digest"));
    }
}

//! Ingest a terminal command execution into the graph (zero-token).
//!
//! Produces an `execution` node, `modified` edges to changed artifacts, and —
//! when the command failed — `failed_on` edges to files named in the output's
//! stack trace / error lines.

use regex::Regex;

use crate::error::Result;
use crate::graph::GraphStore;
use crate::ingest::{clamp, normalize_path, short_hash};
use crate::model::{Edge, Node, NodeType, RelationType};

/// A captured command run from the terminal/telemetry stream.
#[derive(Debug, Clone)]
pub struct ExecutionRecord {
    pub command: String,
    pub exit_code: i32,
    pub output: String,
    pub cwd: Option<String>,
    /// Files observed as changed during the command window.
    pub changed_files: Vec<String>,
    /// Unix seconds; caller supplies the authoritative timestamp.
    pub timestamp: i64,
}

/// Parse `path:line` and `File "x.py", line N` references out of tool output.
pub fn parse_error_locations(output: &str) -> Vec<(String, u32)> {
    let mut out = Vec::new();

    // path/to/file.ext:line   (Rust, JS, Go, generic)
    if let Ok(re) = Regex::new(r"([A-Za-z0-9_./\\-]+\.[A-Za-z]{1,6}):(\d+)") {
        for caps in re.captures_iter(output) {
            if let (Some(p), Some(l)) = (caps.get(1), caps.get(2)) {
                if let Ok(line) = l.as_str().parse::<u32>() {
                    out.push((normalize_path(p.as_str()), line));
                }
            }
        }
    }
    // Python: File "path", line N
    if let Ok(re) = Regex::new(r#"File "([^"]+)", line (\d+)"#) {
        for caps in re.captures_iter(output) {
            if let (Some(p), Some(l)) = (caps.get(1), caps.get(2)) {
                if let Ok(line) = l.as_str().parse::<u32>() {
                    out.push((normalize_path(p.as_str()), line));
                }
            }
        }
    }

    out.sort();
    out.dedup();
    out
}

/// Ingest one execution record. Returns counts + created node ids.
pub fn ingest_execution(
    store: &GraphStore,
    rec: &ExecutionRecord,
    now: i64,
) -> Result<crate::graph::WriteOutcome> {
    let mut outcome = crate::graph::WriteOutcome::default();

    let exec_id = format!(
        "execution:{}",
        short_hash(&format!("{}|{}", rec.command, rec.timestamp))
    );
    let label = clamp(&rec.command, 80);
    let mut content = format!("exit={}\n", rec.exit_code);
    if let Some(cwd) = &rec.cwd {
        content.push_str(&format!("cwd={cwd}\n"));
    }
    content.push_str(&clamp(&rec.output, 2000));

    let exec_node =
        Node::new(&exec_id, NodeType::Execution, label, rec.timestamp).with_content(content);
    if store.upsert_node(&exec_node)? {
        outcome.nodes_created += 1;
    }
    outcome.node_ids.push(exec_id.clone());

    // modified edges to changed artifacts
    for file in &rec.changed_files {
        let path = normalize_path(file);
        let art_id = format!("artifact:{path}");
        let art = Node::new(&art_id, NodeType::Artifact, path.clone(), now);
        if store.upsert_node(&art)? {
            outcome.nodes_created += 1;
        }
        let edge = Edge::new(&exec_id, &art_id, RelationType::Modified, now);
        if store.upsert_edge(&edge)? {
            outcome.edges_created += 1;
        }
    }

    // failed_on edges when the command errored
    if rec.exit_code != 0 {
        for (path, _line) in parse_error_locations(&rec.output) {
            let art_id = format!("artifact:{path}");
            let art = Node::new(&art_id, NodeType::Artifact, path.clone(), now);
            if store.upsert_node(&art)? {
                outcome.nodes_created += 1;
            }
            let edge = Edge::new(&exec_id, &art_id, RelationType::FailedOn, now);
            if store.upsert_edge(&edge)? {
                outcome.edges_created += 1;
            }
        }
    }

    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_generic_and_python_locations() {
        let out = "error at src/auth.ts:42\n  File \"app/main.py\", line 7";
        let locs = parse_error_locations(out);
        assert!(locs.contains(&("src/auth.ts".to_string(), 42)));
        assert!(locs.contains(&("app/main.py".to_string(), 7)));
    }
}

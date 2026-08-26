//! Ingest one agent tool call into the graph (zero-token, ADR-0127).
//!
//! Passive evidence of what an agent actually ran, distinct from `execution`
//! (the terminal's own observed result): this is the tool name and a bounded
//! excerpt of its arguments, captured at the same seam the Git/terminal
//! sensors already occupy, never self-reported by the agent.

use std::sync::OnceLock;

use regex::Regex;

use crate::error::Result;
use crate::graph::GraphStore;
use crate::ingest::{clamp, short_hash};
use crate::model::{Node, NodeType};

/// One captured tool call. `argument_excerpt` is whatever the caller judges
/// worth keeping for this tool -- for a terminal-executing tool, the command
/// string itself.
#[derive(Debug, Clone)]
pub struct ToolInvocationRecord {
    pub tool_name: String,
    pub argument_excerpt: String,
    /// Unix seconds; caller supplies the authoritative timestamp.
    pub timestamp: i64,
}

/// A banned shell-plumbing shape a command line was classified against
/// (ADR-0127 decision 3). Deliberately a short, named list rather than a
/// free-form string: the pattern set is meant to grow, not to be reworded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellHygieneViolation {
    /// A PowerShell-native cmdlet piped around a native command (git, cargo,
    /// npm, node, ...) -- the class of bug that has repeatedly re-encoded
    /// output or silently swallowed a real exit code in this repository.
    PipedPowerShellCmdlet,
    /// `$LASTEXITCODE` read directly instead of trusting the command's own
    /// reported result.
    LastExitCode,
}

impl ShellHygieneViolation {
    pub fn as_str(&self) -> &'static str {
        match self {
            ShellHygieneViolation::PipedPowerShellCmdlet => "piped_powershell_cmdlet",
            ShellHygieneViolation::LastExitCode => "last_exit_code",
        }
    }
}

/// Commands this repository's own conventions treat as native/cross-platform.
const NATIVE_COMMANDS: &[&str] = &[
    "git", "cargo", "npm", "npx", "node", "python", "python3", "dotnet", "gh", "yarn", "pnpm",
    "make",
];

/// PowerShell cmdlets known to re-encode or otherwise mishandle a native
/// command's stdout/stderr when piped directly onto it (the recurring failure
/// this ADR exists to make auditable, not merely documented).
const POWERSHELL_CMDLETS: &[&str] = &[
    "select-object",
    "select-string",
    "foreach-object",
    "where-object",
    "get-childitem",
    "remove-item",
    "test-path",
    "format-table",
    "format-list",
    "sort-object",
    "group-object",
    "measure-object",
    "out-string",
    "tee-object",
    "get-content",
    "set-content",
    "new-item",
    "copy-item",
    "move-item",
    "rename-item",
];

fn first_token(segment: &str) -> Option<&str> {
    segment.split_whitespace().next()
}

fn is_native_command(token: &str) -> bool {
    let bare = token
        .trim_start_matches('&')
        .trim_matches(['"', '\''])
        .to_ascii_lowercase();
    NATIVE_COMMANDS
        .iter()
        .any(|native| bare == *native || bare == format!("{native}.exe"))
}

fn is_powershell_cmdlet(token: &str) -> bool {
    let bare = token.trim_matches(['"', '\'']).to_ascii_lowercase();
    POWERSHELL_CMDLETS.contains(&bare.as_str())
}

fn last_exit_code_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\$LASTEXITCODE")
            .expect("$LASTEXITCODE pattern is a compile-time constant, covered by unit tests")
    })
}

/// Classify one command line against the committed, versioned banned-shape
/// list (ADR-0127 decision 3). Pattern matching only -- no model call, ever,
/// on this path. `None` means the command did not match anything on the list,
/// not that it is provably safe.
pub fn classify_command(command: &str) -> Option<ShellHygieneViolation> {
    if last_exit_code_re().is_match(command) {
        return Some(ShellHygieneViolation::LastExitCode);
    }

    let segments: Vec<&str> = command.split('|').collect();
    if segments.len() < 2 {
        return None;
    }
    let has_native = segments
        .iter()
        .filter_map(|segment| first_token(segment))
        .any(is_native_command);
    let has_cmdlet = segments
        .iter()
        .skip(1) // the piped-onto side is never the first segment
        .filter_map(|segment| first_token(segment))
        .any(is_powershell_cmdlet);
    if has_native && has_cmdlet {
        return Some(ShellHygieneViolation::PipedPowerShellCmdlet);
    }
    None
}

/// Ingest one tool-invocation record. Returns counts + created node ids, with
/// `WriteOutcome::violation` set when `classify_command` found a match against
/// `rec.argument_excerpt`.
pub fn ingest_tool_invocation(
    store: &GraphStore,
    rec: &ToolInvocationRecord,
) -> Result<crate::graph::WriteOutcome> {
    let invocation_id = format!(
        "tool_invocation:{}",
        short_hash(&format!(
            "{}|{}|{}",
            rec.tool_name, rec.argument_excerpt, rec.timestamp
        ))
    );
    let violation = classify_command(&rec.argument_excerpt);
    let label = clamp(&rec.argument_excerpt, 80);
    let mut content = format!("tool={}\n", rec.tool_name);
    if let Some(violation) = violation {
        content.push_str(&format!("violation={}\n", violation.as_str()));
    }
    content.push_str(&clamp(&rec.argument_excerpt, 2000));

    let node = Node::new(
        &invocation_id,
        NodeType::ToolInvocation,
        label,
        rec.timestamp,
    )
    .with_content(content);

    let mut outcome = store.upsert_facts(&[node], &[])?;
    outcome.node_ids.push(invocation_id);
    outcome.violation = violation.map(|v| v.as_str().to_string());
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_piped_native_command_and_cmdlet_is_flagged() {
        for command in [
            "cargo test | Select-Object -Last 20",
            "git log --oneline | Select-String -Pattern foo",
            "npm test 2>&1 | Select-String -Pattern FAIL",
            "node script.mjs | ForEach-Object { $_ }",
        ] {
            assert_eq!(
                classify_command(command),
                Some(ShellHygieneViolation::PipedPowerShellCmdlet),
                "expected a violation for: {command}"
            );
        }
    }

    #[test]
    fn dollar_lastexitcode_is_flagged_regardless_of_case() {
        for command in [
            "node build.js; Write-Output $LASTEXITCODE",
            "cargo build; echo $lastexitcode",
        ] {
            assert_eq!(
                classify_command(command),
                Some(ShellHygieneViolation::LastExitCode),
                "expected a violation for: {command}"
            );
        }
    }

    #[test]
    fn ordinary_commands_are_not_flagged() {
        for command in [
            "git status --short --branch",
            "cargo test -p ackplane-bridge",
            "node scripts/canonical-push.mjs",
            "npm --prefix editors/vscode run compile",
            "git log --oneline -5",
        ] {
            assert_eq!(
                classify_command(command),
                None,
                "expected no violation for: {command}"
            );
        }
    }

    #[test]
    fn a_pipe_with_no_native_command_on_either_side_is_not_flagged() {
        // A cmdlet piped to another cmdlet involves no native command at all --
        // outside this ADR's stated scope (a native command's output being
        // re-encoded), so it is left unclassified rather than over-reaching.
        assert_eq!(classify_command("Get-Process | Sort-Object CPU"), None);
    }

    #[test]
    fn a_pipe_with_a_native_command_only_on_the_piped_onto_side_is_not_flagged() {
        // The native command must be the thing PRODUCING output that a cmdlet
        // then mangles; a cmdlet piped into a native command is a different
        // (and not yet documented) shape.
        assert_eq!(
            classify_command("Get-ChildItem | git check-ignore --stdin"),
            None
        );
    }

    #[test]
    fn ingest_creates_a_tool_invocation_node_and_reports_a_violation() {
        let store = GraphStore::new(crate::db::open_in_memory().unwrap());
        let record = ToolInvocationRecord {
            tool_name: "run_in_terminal".to_string(),
            argument_excerpt: "cargo test | Select-Object -Last 5".to_string(),
            timestamp: 1_000,
        };
        let outcome = ingest_tool_invocation(&store, &record).unwrap();
        assert_eq!(outcome.nodes_created, 1);
        assert_eq!(
            outcome.violation.as_deref(),
            Some("piped_powershell_cmdlet")
        );
        let node_id = outcome
            .node_ids
            .iter()
            .find(|id| id.starts_with("tool_invocation:"))
            .expect("a tool_invocation node id is reported");
        let node = store
            .get_node(node_id)
            .unwrap()
            .expect("the node was actually written");
        assert_eq!(node.node_type, NodeType::ToolInvocation);
    }

    #[test]
    fn ingest_reports_no_violation_for_a_clean_command() {
        let store = GraphStore::new(crate::db::open_in_memory().unwrap());
        let record = ToolInvocationRecord {
            tool_name: "run_in_terminal".to_string(),
            argument_excerpt: "git status --short".to_string(),
            timestamp: 1_000,
        };
        let outcome = ingest_tool_invocation(&store, &record).unwrap();
        assert_eq!(outcome.violation, None);
    }

    #[test]
    fn repeat_ingestion_of_the_identical_record_is_idempotent() {
        let store = GraphStore::new(crate::db::open_in_memory().unwrap());
        let record = ToolInvocationRecord {
            tool_name: "run_in_terminal".to_string(),
            argument_excerpt: "git status".to_string(),
            timestamp: 1_000,
        };
        ingest_tool_invocation(&store, &record).unwrap();
        let second = ingest_tool_invocation(&store, &record).unwrap();
        // Same content at the same timestamp hashes to the same id, so a
        // second capture of the identical call upserts rather than duplicates.
        assert_eq!(second.nodes_created, 0);
    }
}

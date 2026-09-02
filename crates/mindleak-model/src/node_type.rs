//! Kind of entity a graph node represents (ADR-0140 decision 5).
//!
//! Lives here rather than in `mindleak-core` so the discrimination logic in
//! [`crate::discrimination`] -- which ranks candidates *by* `NodeType` -- can
//! be shared with Ackplane without pulling all of `mindleak-core` across the
//! federation boundary (ADR-0082). `mindleak-core::model` re-exports this type
//! so its own call sites are unaffected.

use serde::{Deserialize, Serialize};

/// Kind of entity a node represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    /// AST-extracted function, class, interface, etc.
    Symbol,
    /// Workspace file, config, directory, test suite.
    Artifact,
    /// Terminal command / process run with an exit code.
    Execution,
    /// One agent tool call, captured passively (ADR-0127): the tool name and
    /// a bounded excerpt of its arguments -- for a terminal-executing tool,
    /// the command string itself. Distinct from `Execution`: this is about
    /// what the agent asked to run, not the terminal's own observed result.
    ToolInvocation,
    /// High-level human/agent intent: commit, decision, tradeoff.
    Intent,
    /// An AI agent / client session (optional attribution).
    Agent,
    /// External dependency referenced by a bare import specifier.
    Package,
    /// A compiled, regenerable rendering of current graph state (ADR-0101):
    /// playbook, runbook, weekly report, etc. Content is always regenerated
    /// output, never hand-authored or hand-edited (ADR-0056's precedent).
    Digest,
}

impl NodeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeType::Symbol => "symbol",
            NodeType::Artifact => "artifact",
            NodeType::Execution => "execution",
            NodeType::ToolInvocation => "tool_invocation",
            NodeType::Intent => "intent",
            NodeType::Agent => "agent",
            NodeType::Package => "package",
            NodeType::Digest => "digest",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "symbol" => Some(NodeType::Symbol),
            "artifact" => Some(NodeType::Artifact),
            "execution" => Some(NodeType::Execution),
            "tool_invocation" => Some(NodeType::ToolInvocation),
            "intent" => Some(NodeType::Intent),
            "agent" => Some(NodeType::Agent),
            "package" => Some(NodeType::Package),
            "digest" => Some(NodeType::Digest),
            _ => None,
        }
    }
}

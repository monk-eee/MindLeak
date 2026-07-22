//! Node/edge domain model for the temporal context graph.

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
    /// High-level human/agent intent: commit, decision, tradeoff.
    Intent,
    /// An AI agent / client session (optional attribution).
    Agent,
}

impl NodeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeType::Symbol => "symbol",
            NodeType::Artifact => "artifact",
            NodeType::Execution => "execution",
            NodeType::Intent => "intent",
            NodeType::Agent => "agent",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "symbol" => Some(NodeType::Symbol),
            "artifact" => Some(NodeType::Artifact),
            "execution" => Some(NodeType::Execution),
            "intent" => Some(NodeType::Intent),
            "agent" => Some(NodeType::Agent),
            _ => None,
        }
    }
}

/// Directional relationship between two nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationType {
    /// Execution changed an artifact.
    Modified,
    /// Execution failed on a symbol/artifact (from a stack trace).
    FailedOn,
    /// Symbol calls another symbol.
    Calls,
    /// Intent refactored a symbol/artifact.
    Refactored,
    /// Weak semantic association.
    RelatesTo,
    /// Artifact contains a symbol.
    Contains,
    /// An agent ingested or focused this node (attribution; decays).
    Observed,
}

impl RelationType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RelationType::Modified => "modified",
            RelationType::FailedOn => "failed_on",
            RelationType::Calls => "calls",
            RelationType::Refactored => "refactored",
            RelationType::RelatesTo => "relates_to",
            RelationType::Contains => "contains",
            RelationType::Observed => "observed",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "modified" => Some(RelationType::Modified),
            "failed_on" => Some(RelationType::FailedOn),
            "calls" => Some(RelationType::Calls),
            "refactored" => Some(RelationType::Refactored),
            "relates_to" => Some(RelationType::RelatesTo),
            "contains" => Some(RelationType::Contains),
            "observed" => Some(RelationType::Observed),
            _ => None,
        }
    }

    /// Default decay half-life (hours) for edges created from this relation.
    /// Raw execution evidence decays fast; human intent lingers.
    pub fn default_half_life_hours(&self) -> f64 {
        match self {
            RelationType::Modified | RelationType::FailedOn => 24.0,
            RelationType::Calls | RelationType::Contains | RelationType::Refactored => 168.0,
            RelationType::RelatesTo | RelationType::Observed => 48.0,
        }
    }
}

/// A graph node as stored in SQLite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    #[serde(rename = "type")]
    pub node_type: NodeType,
    pub label: String,
    pub content: Option<String>,
    pub created_at: i64,
    pub last_accessed_at: i64,
}

impl Node {
    pub fn new(
        id: impl Into<String>,
        node_type: NodeType,
        label: impl Into<String>,
        now: i64,
    ) -> Self {
        Node {
            id: id.into(),
            node_type,
            label: label.into(),
            content: None,
            created_at: now,
            last_accessed_at: now,
        }
    }

    pub fn with_content(mut self, content: impl Into<String>) -> Self {
        self.content = Some(content.into());
        self
    }
}

/// A graph edge as stored in SQLite (base weight; effective weight is derived).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub source_id: String,
    pub target_id: String,
    pub relation: RelationType,
    pub weight: f64,
    pub half_life_hours: f64,
    pub updated_at: i64,
}

impl Edge {
    pub fn new(
        source_id: impl Into<String>,
        target_id: impl Into<String>,
        relation: RelationType,
        now: i64,
    ) -> Self {
        Edge {
            source_id: source_id.into(),
            target_id: target_id.into(),
            relation,
            weight: 1.0,
            half_life_hours: relation.default_half_life_hours(),
            updated_at: now,
        }
    }
}

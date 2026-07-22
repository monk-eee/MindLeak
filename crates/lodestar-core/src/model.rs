//! Domain model for the Lodestar Intent Plane: goals (the constitution), tasks
//! (the executive), conformance verdicts, and consolidated learned knowledge.

use serde::{Deserialize, Serialize};

/// What a goal expresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GoalKind {
    /// A thing to achieve.
    Objective,
    /// A boundary that must hold.
    Constraint,
    /// A load-bearing rule that must never be violated.
    Invariant,
}

impl GoalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            GoalKind::Objective => "objective",
            GoalKind::Constraint => "constraint",
            GoalKind::Invariant => "invariant",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "objective" => Some(GoalKind::Objective),
            "constraint" => Some(GoalKind::Constraint),
            "invariant" => Some(GoalKind::Invariant),
            _ => None,
        }
    }

    /// Constraints and invariants are what conformance checks against.
    pub fn is_normative(&self) -> bool {
        matches!(self, GoalKind::Constraint | GoalKind::Invariant)
    }
}

/// Lifecycle of a goal version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GoalStatus {
    Draft,
    Active,
    Superseded,
}

impl GoalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            GoalStatus::Draft => "draft",
            GoalStatus::Active => "active",
            GoalStatus::Superseded => "superseded",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(GoalStatus::Draft),
            "active" => Some(GoalStatus::Active),
            "superseded" => Some(GoalStatus::Superseded),
            _ => None,
        }
    }
}

/// Lifecycle of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Open,
    Claimed,
    InReview,
    Done,
    Blocked,
    Abandoned,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Open => "open",
            TaskStatus::Claimed => "claimed",
            TaskStatus::InReview => "in_review",
            TaskStatus::Done => "done",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Abandoned => "abandoned",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "open" => Some(TaskStatus::Open),
            "claimed" => Some(TaskStatus::Claimed),
            "in_review" => Some(TaskStatus::InReview),
            "done" => Some(TaskStatus::Done),
            "blocked" => Some(TaskStatus::Blocked),
            "abandoned" => Some(TaskStatus::Abandoned),
            _ => None,
        }
    }
}

/// The outcome of a conformance check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The change is sanctioned and consistent with governing intent.
    Aligned,
    /// Governed code changed without a covering task (unsanctioned).
    Drift,
    /// The change contradicts a constraint/invariant.
    Violation,
    /// A semantic check could not decide; a human should look.
    NeedsHuman,
}

/// How an active goal governs a linked MindLeak code node (ADR-0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeBindingMode {
    Governed,
    ForbidChange,
}

impl CodeBindingMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            CodeBindingMode::Governed => "governed",
            CodeBindingMode::ForbidChange => "forbid_change",
        }
    }

    pub fn from_tag(value: &str) -> Option<Self> {
        match value {
            "governed" => Some(CodeBindingMode::Governed),
            "forbid_change" => Some(CodeBindingMode::ForbidChange),
            _ => None,
        }
    }
}

/// An active goal plus the policy governing one linked code node.
#[derive(Debug, Clone)]
pub struct CodeBinding {
    pub goal: Goal,
    pub mode: CodeBindingMode,
}

/// One MindLeak graph fact supporting an evidence claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceProvenance {
    pub source_id: String,
    pub target_id: String,
    pub relation: String,
}

/// Versioned evidence received across the loose MindLeak/Lodestar seam.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConformanceEvidence {
    pub schema_version: u32,
    pub task_id: Option<String>,
    pub agent_id: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub changed_node_ids: Vec<String>,
    pub failed_node_ids: Vec<String>,
    pub execution_ids: Vec<String>,
    pub successful_execution_ids: Vec<String>,
    pub commit_ids: Vec<String>,
    pub summary: String,
    pub provenance: Vec<EvidenceProvenance>,
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Aligned => "aligned",
            Verdict::Drift => "drift",
            Verdict::Violation => "violation",
            Verdict::NeedsHuman => "needs_human",
        }
    }
}

/// A goal row: a unit of the constitution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub slug: String,
    pub kind: GoalKind,
    pub title: String,
    pub statement: String,
    pub status: GoalStatus,
    pub version: i64,
    pub parent_id: Option<String>,
    pub superseded_by: Option<String>,
    pub reason: Option<String>,
    pub created_at: i64,
}

/// A task row: a unit of claimable work serving a goal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub goal_id: String,
    pub parent_task_id: Option<String>,
    pub title: String,
    pub acceptance: String,
    pub status: TaskStatus,
    pub owner: Option<String>,
    pub claim_started_at: Option<i64>,
    pub lease_expires_at: Option<i64>,
    pub blocked_by: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

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

/// The result of a conformance check (returned to callers; also audited).
#[derive(Debug, Clone, Serialize)]
pub struct ConformanceResult {
    pub verdict: Verdict,
    pub findings: Vec<String>,
}

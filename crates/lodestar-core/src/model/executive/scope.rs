//! Advisory claim scope and the pre-flight overlap it enables.

use serde::{Deserialize, Serialize};

/// Optional paths and symbol ids an agent declares when claiming work
/// (ADR-0024). Paths are normalized workspace-relative glob patterns; symbols
/// are opaque MindLeak `symbol:` ids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskScope {
    pub paths: Vec<String>,
    pub symbols: Vec<String>,
}

/// How much an intersecting claim actually costs, from the branches the two
/// sessions declared (ADR-0035 heuristic 4).
///
/// An intersection is not one risk. Two agents editing a path on the same branch
/// are colliding *now*; on different branches they are building a merge conflict
/// for later. Reporting both as "overlap" is what made the advice easy to
/// ignore, because the caller had to guess which one it had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlapSignal {
    /// Both sessions declared the same branch: the edits land in one history.
    SameBranchCollision,
    /// The sessions declared different branches: divergence, paid at merge.
    CrossBranchMergeRisk,
    /// At least one side declared no branch, so the distinction is unknown.
    /// Declared context is self-reported and optional (ADR-0035 decision 5);
    /// absence degrades the signal, and must never be read as either verdict.
    Undeclared,
}

impl OverlapSignal {
    /// Classify one intersection from the two declared branches.
    pub fn classify(requester: Option<&str>, owner: Option<&str>) -> Self {
        match (requester, owner) {
            (Some(requester), Some(owner)) if requester == owner => Self::SameBranchCollision,
            (Some(_), Some(_)) => Self::CrossBranchMergeRisk,
            _ => Self::Undeclared,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SameBranchCollision => "same_branch_collision",
            Self::CrossBranchMergeRisk => "cross_branch_merge_risk",
            Self::Undeclared => "undeclared",
        }
    }
}

/// One active claim whose declared scope intersects a pre-flight request.
/// Advisory only: this reports ownership intent and never grants a lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimOverlap {
    pub task_id: String,
    pub owner: String,
    pub lease_expires_at: i64,
    pub scope: TaskScope,
    pub matching_paths: Vec<String>,
    pub matching_symbols: Vec<String>,
    /// The branch the owning session declared at `open_session`, if any.
    pub owner_branch: Option<String>,
    pub signal: OverlapSignal,
}

/// The result of one pre-flight overlap check.
///
/// `requester_branch` is the branch the *asking* session declared, echoed back
/// because it is half of every `signal`. Without it an `undeclared` result is
/// ambiguous — the caller cannot tell whether the peer said nothing or it did
/// itself — and a stale declaration of its own stays invisible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimOverlapReport {
    pub requester_branch: Option<String>,
    pub claims: Vec<ClaimOverlap>,
}

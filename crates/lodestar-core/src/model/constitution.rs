//! Constitutional types: goals and clauses, their status, and the code
//! bindings that say which artefacts a clause governs.

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
    /// A broad decision rule. Normative but ambiguous cases route to review,
    /// never an automatic hard block (SPEC-CONSTITUTION §4).
    Principle,
}

impl GoalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            GoalKind::Objective => "objective",
            GoalKind::Constraint => "constraint",
            GoalKind::Invariant => "invariant",
            GoalKind::Principle => "principle",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "objective" => Some(GoalKind::Objective),
            "constraint" => Some(GoalKind::Constraint),
            "invariant" => Some(GoalKind::Invariant),
            "principle" => Some(GoalKind::Principle),
            _ => None,
        }
    }

    /// Constraints and invariants are what conformance checks against.
    pub fn is_normative(&self) -> bool {
        matches!(self, GoalKind::Constraint | GoalKind::Invariant)
    }
}

/// The proportional outcome when a clause is not met (SPEC-CONSTITUTION §4/§8):
/// uncertainty asks for review, only a specific active clause with adequate
/// evidence can hard-block. Ordered by severity: `advise < review < block`, so
/// the ADR-0034 ceiling rule can take a minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Consequence {
    /// Surface guidance; never blocks.
    Advise,
    /// Route to human review.
    Review,
    /// Hard policy; may block with adequate evidence.
    Block,
}

impl Consequence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Consequence::Advise => "advise",
            Consequence::Review => "review",
            Consequence::Block => "block",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "advise" => Some(Consequence::Advise),
            "review" => Some(Consequence::Review),
            "block" => Some(Consequence::Block),
            _ => None,
        }
    }
}

/// Where a clause came from (SPEC-CONSTITUTION §10 `ClauseSource.origin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClauseOrigin {
    /// Authored directly in this project.
    Local,
    /// Adopted from an immutable policy pack.
    Pack,
    /// Derived from a cited repository fact.
    Discovered,
}

impl ClauseOrigin {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClauseOrigin::Local => "local",
            ClauseOrigin::Pack => "pack",
            ClauseOrigin::Discovered => "discovered",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "local" => Some(ClauseOrigin::Local),
            "pack" => Some(ClauseOrigin::Pack),
            "discovered" => Some(ClauseOrigin::Discovered),
            _ => None,
        }
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

/// How an active goal governs a linked MindLeak artifact node (ADR-0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactBindingMode {
    Governed,
    ForbidChange,
}

impl ArtifactBindingMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ArtifactBindingMode::Governed => "governed",
            ArtifactBindingMode::ForbidChange => "forbid_change",
        }
    }

    pub fn from_tag(value: &str) -> Option<Self> {
        match value {
            "governed" => Some(ArtifactBindingMode::Governed),
            "forbid_change" => Some(ArtifactBindingMode::ForbidChange),
            _ => None,
        }
    }
}

/// An active goal plus the policy governing one linked artifact node.
#[derive(Debug, Clone, Serialize)]
pub struct ArtifactBinding {
    pub goal: Goal,
    pub mode: ArtifactBindingMode,
}

/// One immutable constitutional version: the frozen preamble and clause set
/// that authorises verdicts (SPEC-CONSTITUTION §10). An amendment writes a new
/// version; prior conformance records retain the version they were judged
/// under. Migration does not invent a purpose, preamble, or authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstitutionVersion {
    pub id: String,
    pub version: i64,
    pub project_identity: Option<String>,
    pub purpose: Option<String>,
    pub preamble: Option<String>,
    pub status: GoalStatus,
    pub created_by: Option<String>,
    pub created_at: i64,
    pub activated_by: Option<String>,
    pub activated_at: Option<i64>,
}

/// Whether a project has adopted a constitution at all
/// (SPEC-CONSTITUTION §11). Reported rather than inferred, so an agent can tell
/// "no policy exists" apart from "policy exists and permits this".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConstitutionState {
    /// No constitutional version exists; conformance can only defer to a human.
    Absent,
    /// A version is drafted but not activated, so it authorises nothing yet.
    Draft,
    /// An activated version governs work.
    Active,
}

impl ConstitutionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConstitutionState::Absent => "absent",
            ConstitutionState::Draft => "draft",
            ConstitutionState::Active => "active",
        }
    }
}

/// The adoption state of the local constitution: which lifecycle stage it is
/// in, the version that stage refers to, and how many clauses it carries. A
/// draft reports its own clause count so bootstrap progress is visible without
/// implying the clauses are enforceable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstitutionStatus {
    pub state: ConstitutionState,
    pub version: Option<ConstitutionVersion>,
    pub clause_count: i64,
}

/// A bootstrap proposal (SPEC-CONSTITUTION 7.3): a drafted version, the cited
/// repository facts grounding it, and the Common Core clauses awaiting an
/// adopt/tailor/reject disposition. Nothing here governs anything — the draft
/// authorises no verdict until it is explicitly activated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstitutionProposal {
    pub version: ConstitutionVersion,
    pub facts: Vec<crate::discovery::ProjectFact>,
    pub common_core: crate::policy::PackProposalBatch,
}

/// A goal row: a clause of the constitution (SPEC-CONSTITUTION §10). The
/// enforcement fields (`scope`, `evidence_contract`, `consequence`) stay absent
/// until explicitly completed; an incomplete clause is review-only and can
/// never drive a hard verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// The constitutional version this clause belongs to, if any.
    pub constitution_version: Option<String>,
    /// Why the clause exists (distinct from `reason`, the amendment note).
    pub rationale: Option<String>,
    /// The declared scope in which the clause applies.
    pub scope: Option<String>,
    /// The evidence contract that satisfies the clause.
    pub evidence_contract: Option<String>,
    /// The proportional consequence of non-conformance.
    pub consequence: Option<Consequence>,
    /// Whether a bounded waiver may suspend the clause.
    pub waivable: bool,
    /// The authority required to waive the clause.
    pub waiver_authority: Option<String>,
    /// The provenance of the clause.
    pub origin: ClauseOrigin,
}

impl Goal {
    /// A clause can drive a hard verdict only once it declares a scope, an
    /// evidence contract, and a consequence. Until then it is review-only
    /// (SPEC-CONSTITUTION §10: incomplete clauses guide review, never block).
    pub fn is_enforceable(&self) -> bool {
        self.scope.is_some() && self.evidence_contract.is_some() && self.consequence.is_some()
    }
}

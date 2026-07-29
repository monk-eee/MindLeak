//! Conformance: the evidence a change carries, the verdict it earns, and the
//! durable record that makes the verdict resolvable after the fact.

use serde::{Deserialize, Serialize};

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

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Aligned => "aligned",
            Verdict::Drift => "drift",
            Verdict::Violation => "violation",
            Verdict::NeedsHuman => "needs_human",
        }
    }

    pub fn from_tag(value: &str) -> Option<Self> {
        match value {
            "aligned" => Some(Verdict::Aligned),
            "drift" => Some(Verdict::Drift),
            "violation" => Some(Verdict::Violation),
            "needs_human" => Some(Verdict::NeedsHuman),
            _ => None,
        }
    }
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

/// A one-glance summary of the conformance record a task closed on.
///
/// A task reaching `done` says nothing about whether its evidence ever affirmed
/// the work. Measured over this repository, 57 of 101 `done` tasks rested on a
/// `drift`/`needs_human` verdict or on an `aligned` one covering no nodes at
/// all, and every one of them read on the board exactly like a task whose
/// evidence proved something. `affirms` is the distinction, carried where the
/// completion is reported rather than left for a reader to reconstruct from the
/// conformance chain.
///
/// Derived at read time from the durable record; nothing here is stored twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TaskReceipt {
    /// The conformance record this summarises, resolvable after the fact.
    pub conformance_id: i64,
    pub verdict: Verdict,
    /// How many nodes the evidence bundle actually covered.
    pub covered_nodes: usize,
    pub checked_at: i64,
    /// Whether the receipt affirmed the work: `aligned` **and** covering at
    /// least one node. An `aligned` verdict over an empty bundle is agreement
    /// about nothing, which is not the same as proof.
    pub affirms: bool,
}

/// One persisted conformance audit record: the durable, resolvable evidence
/// link for a task. Its `id` is stable and addressable after the fact, and the
/// stored `evidence` is exactly the bundle that produced `verdict`/`findings`.
#[derive(Debug, Clone, Serialize)]
pub struct ConformanceRecord {
    pub id: i64,
    pub task_id: Option<String>,
    pub evidence_schema_version: u32,
    pub evidence: String,
    pub verdict: Verdict,
    pub findings: String,
    pub checked_at: i64,
}

/// An authoritative conformance preflight. `complete_task` consumes this exact
/// persisted result and rejects it if the evidence or relevant intent state has
/// changed, so an optional semantic judge is never invoked twice for one task
/// transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConformanceCheck {
    pub id: i64,
    pub token: String,
    pub verdict: Verdict,
    pub findings: Vec<String>,
}

/// The result of a conformance check (returned to callers; also audited).
#[derive(Debug, Clone, Serialize)]
pub struct ConformanceResult {
    pub verdict: Verdict,
    pub findings: Vec<String>,
}

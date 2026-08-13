//! Conformance types: what was changed, the verdict on it, and the receipt
//! that proves the judgement was the one acted on.

use super::constitution::{ArtifactBindingMode, Goal};
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

/// The forward-looking disposition returned by `advise` (ADR-0029): a
/// proportional judgment made *before* work is done, from clause resolution
/// alone. It is deliberately not a `Verdict` — advice never records a
/// conformance result and never runs the semantic judge, so it can only surface
/// what governs the intended change, warn about a would-be drift, block on a
/// hard `forbid_change` lock, or defer to a human when no constitution exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdviceDisposition {
    /// Nothing blocks the change; any in-scope governing clauses are surfaced to honour.
    Advise,
    /// The change would drift outside a covering task; get a covering task or review first.
    Review,
    /// A hard `forbid_change` clause locks this code; do not proceed without a waiver.
    Block,
    /// No constitution is adopted (or policy is genuinely ambiguous); a human should look.
    NeedsHuman,
}

impl AdviceDisposition {
    /// The stable snake_case tag, matching the serialized form.
    pub fn as_str(&self) -> &'static str {
        match self {
            AdviceDisposition::Advise => "advise",
            AdviceDisposition::Review => "review",
            AdviceDisposition::Block => "block",
            AdviceDisposition::NeedsHuman => "needs_human",
        }
    }
}

/// The machine-readable cause when advice cannot proceed without a person.
/// Kept separate from [`AdviceDisposition`] so adding diagnostic precision does
/// not break clients that already branch on `needs_human`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdviceReason {
    /// This repository has no active constitutional clauses yet.
    NoConstitutionAdopted,
    /// Active policy exists, but does not determine one safe course of action.
    Ambiguous,
}

/// One active clause governing a node in an intended change scope (ADR-0029).
#[derive(Debug, Clone, Serialize)]
pub struct GoverningClause {
    pub node_id: String,
    pub goal: Goal,
    pub mode: ArtifactBindingMode,
}

/// The result of `advise` (ADR-0029): the active clauses governing an intended
/// change scope plus one proportional disposition. It carries no evidence and
/// records no verdict — a read-only projection of the adopted constitution.
#[derive(Debug, Clone, Serialize)]
pub struct Advice {
    pub disposition: AdviceDisposition,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<AdviceReason>,
    pub governing: Vec<GoverningClause>,
    pub findings: Vec<String>,
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

/// The state a subject's certification holds (ADR-0090).
///
/// Verification is the capability; this is the status it produces. The states
/// are deliberately distinct: none of the ones below `Certified` renders as
/// certified, so a quiet result is never mistaken for a clean one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CertificationState {
    /// Evidence affirmed the subject against the named policy version.
    Certified,
    /// Verified and refused. `reason` names what failed.
    NotCertified,
    /// Excepted by a live waiver, which carries its own expiry and remediation.
    Waived,
    /// The check could not decide; a person has to look.
    NeedsHuman,
    /// No constitution is adopted, so there is nothing to certify against.
    Uncertifiable,
    /// The subject moved past the evidence behind its status.
    Stale,
}

impl CertificationState {
    /// The stable snake_case tag, matching the serialized form.
    pub fn as_str(&self) -> &'static str {
        match self {
            CertificationState::Certified => "certified",
            CertificationState::NotCertified => "not_certified",
            CertificationState::Waived => "waived",
            CertificationState::NeedsHuman => "needs_human",
            CertificationState::Uncertifiable => "uncertifiable",
            CertificationState::Stale => "stale",
        }
    }
}

/// Which clauses a status was judged against, and which it was not (ADR-0090
/// §7). A status covers the clauses it names and nothing more, so the
/// unevaluated set travels beside it rather than being left for a reader to
/// infer from silence.
#[derive(Debug, Clone, Serialize)]
pub struct ClauseCoverage {
    pub evaluated: Vec<String>,
    pub not_evaluated: Vec<String>,
}

/// A qualified certification status (ADR-0090): never a bare badge.
///
/// Every field beside the state is what stops it being read as a framework
/// verdict. `policy_version` is the self-limiting one — "certified against
/// constitution:v3" cannot be cropped into "compliant" — and it is why this
/// type has no variant that asserts external framework compliance.
///
/// Derived at read time from the durable conformance record; nothing here is
/// stored twice.
#[derive(Debug, Clone, Serialize)]
pub struct CertificationStatus {
    pub subject: String,
    /// The commit the evidence was judged over; `None` when it names none.
    pub commit: Option<String>,
    /// The constitution version judged against; `None` when none is adopted.
    pub policy_version: Option<String>,
    /// The conformance record behind the status, resolvable after the fact.
    pub evidence_bundle: Option<i64>,
    /// When the judgement was made.
    pub certified_at: Option<i64>,
    pub state: CertificationState,
    /// Why the status is what it is, in words. Diagnostic, and never the branch
    /// condition for a reader — that is `state`.
    pub reason: String,
    /// How many nodes the evidence behind the status covered.
    pub covered_nodes: usize,
    pub coverage: ClauseCoverage,
    /// The live waiver behind a `waived` state, with its expiry and remediation.
    pub waiver: Option<crate::waiver::Waiver>,
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

/// The compact, tamper-evident handle for a conformance check. New audit rows
/// retain their canonical findings vector, so completion can reload it instead
/// of requiring clients to echo advisory findings that may be too large to send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConformanceCheckReference {
    pub id: i64,
    pub token: String,
}

/// The result of a conformance check (returned to callers; also audited).
#[derive(Debug, Clone, Serialize)]
pub struct ConformanceResult {
    pub verdict: Verdict,
    pub findings: Vec<String>,
}

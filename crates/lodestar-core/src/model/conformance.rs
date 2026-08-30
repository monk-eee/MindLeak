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

/// One active lesson delivered with pre-edit advice because it names a node in
/// the intended change scope.
#[derive(Debug, Clone, Serialize)]
pub struct KnowledgeAdvisory {
    pub id: String,
    pub statement: String,
    pub weight: f64,
    pub confirmed_at: i64,
    pub matched_nodes: Vec<String>,
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
    pub known_context: Vec<KnowledgeAdvisory>,
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
    /// A closed set of Lodestar-internal ledger acts standing in for a
    /// MindLeak node mutation (ADR-0110): entries are `ledger_act:<kind>:<id>`
    /// ids built only by [`crate::Lodestar::ledger_act_evidence`], never
    /// caller-supplied directly. `#[serde(default)]` lets evidence recorded
    /// before this field existed keep deserializing as an empty list.
    #[serde(default)]
    pub ledger_act_ids: Vec<String>,
    pub summary: String,
    pub provenance: Vec<EvidenceProvenance>,
}

/// The closed, enumerated set of Lodestar-internal acts eligible as
/// conformance evidence in their own right (ADR-0110), because each already
/// carries a durably recorded actor and timestamp that
/// [`crate::Lodestar::ledger_act_evidence`] verifies against the current
/// claim -- without any MindLeak call. Adding a new variant is a decision for
/// a future ADR amendment, the same discipline ADR-0009 applies to what may
/// populate `changed_node_ids`; this is deliberately not an escape hatch for
/// "any Lodestar write".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerActKind {
    /// A design item was registered (`design_items.proposed_by`/`created_at`).
    DesignRegistered,
    /// A design item was accepted or rejected
    /// (`design_items.decided_by`/`updated_at`).
    DesignDecided,
    /// A waiver was granted (`waivers.approved_by`/`created_at`).
    WaiverGranted,
    /// A constitution amendment was recorded
    /// (`constitution_amendments.amended_by`/`created_at`).
    ConstitutionAmended,
    /// A clause was retired and replaced
    /// (`goals.superseded_by_agent`/`superseded_at`).
    ///
    /// Added by ADR-0144, which supplied the prerequisite ADR-0110 named:
    /// `supersede_goal` used to record only a free-form reason, so there was no
    /// actor to verify against the claiming agent and wiring it in would have
    /// meant fabricating an attribution. A clause superseded before that
    /// change still has no recorded actor, and is refused rather than guessed.
    GoalSuperseded,
}

impl LedgerActKind {
    /// The stable snake_case tag used in a `ledger_act:<kind>:<id>` node id.
    pub fn as_str(&self) -> &'static str {
        match self {
            LedgerActKind::DesignRegistered => "design_registered",
            LedgerActKind::DesignDecided => "design_decided",
            LedgerActKind::WaiverGranted => "waiver_granted",
            LedgerActKind::ConstitutionAmended => "constitution_amended",
            LedgerActKind::GoalSuperseded => "goal_superseded",
        }
    }

    /// Parse the tag `as_str` produces, for the MCP tool's caller-supplied
    /// `kind` argument. Unlike a `changed_node_id`, this string selects which
    /// store lookup runs, so an unrecognised value must refuse rather than
    /// guess.
    pub fn parse(tag: &str) -> Option<Self> {
        match tag {
            "design_registered" => Some(LedgerActKind::DesignRegistered),
            "design_decided" => Some(LedgerActKind::DesignDecided),
            "waiver_granted" => Some(LedgerActKind::WaiverGranted),
            "constitution_amended" => Some(LedgerActKind::ConstitutionAmended),
            "goal_superseded" => Some(LedgerActKind::GoalSuperseded),
            _ => None,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_act_kind_as_str_matches_the_serialized_snake_case_tag() {
        assert_eq!(
            LedgerActKind::DesignRegistered.as_str(),
            "design_registered"
        );
        assert_eq!(LedgerActKind::DesignDecided.as_str(), "design_decided");
        assert_eq!(LedgerActKind::WaiverGranted.as_str(), "waiver_granted");
        assert_eq!(
            LedgerActKind::ConstitutionAmended.as_str(),
            "constitution_amended"
        );
    }

    #[test]
    fn ledger_act_kind_parse_round_trips_every_as_str_output_and_refuses_an_unknown_tag() {
        for kind in [
            LedgerActKind::DesignRegistered,
            LedgerActKind::DesignDecided,
            LedgerActKind::WaiverGranted,
            LedgerActKind::ConstitutionAmended,
        ] {
            assert_eq!(LedgerActKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(LedgerActKind::parse("not_a_real_ledger_act"), None);
    }

    #[test]
    fn advice_disposition_as_str_matches_the_serialized_snake_case_tag() {
        assert_eq!(AdviceDisposition::Advise.as_str(), "advise");
        assert_eq!(AdviceDisposition::Review.as_str(), "review");
        assert_eq!(AdviceDisposition::Block.as_str(), "block");
        assert_eq!(AdviceDisposition::NeedsHuman.as_str(), "needs_human");
    }

    #[test]
    fn verdict_as_str_matches_the_serialized_snake_case_tag() {
        assert_eq!(Verdict::Aligned.as_str(), "aligned");
        assert_eq!(Verdict::Drift.as_str(), "drift");
        assert_eq!(Verdict::Violation.as_str(), "violation");
        assert_eq!(Verdict::NeedsHuman.as_str(), "needs_human");
    }

    #[test]
    fn verdict_from_tag_round_trips_every_as_str_output_and_refuses_an_unknown_tag() {
        for verdict in [
            Verdict::Aligned,
            Verdict::Drift,
            Verdict::Violation,
            Verdict::NeedsHuman,
        ] {
            assert_eq!(Verdict::from_tag(verdict.as_str()), Some(verdict));
        }
        assert_eq!(Verdict::from_tag("not_a_real_verdict"), None);
    }

    #[test]
    fn certification_state_as_str_matches_the_serialized_snake_case_tag() {
        assert_eq!(CertificationState::Certified.as_str(), "certified");
        assert_eq!(CertificationState::NotCertified.as_str(), "not_certified");
        assert_eq!(CertificationState::Waived.as_str(), "waived");
        assert_eq!(CertificationState::NeedsHuman.as_str(), "needs_human");
        assert_eq!(CertificationState::Uncertifiable.as_str(), "uncertifiable");
        assert_eq!(CertificationState::Stale.as_str(), "stale");
    }
}

//! Typed controls and enforcement ceilings (ADR-0034, SPEC-CONSTITUTION §4).
//!
//! A clause carries legitimacy — rationale, scope, evidence contract,
//! consequence, waiver policy. A **control** is the mechanism that reports
//! whether the clause was met. Keeping them separate is what lets a hotfix be an
//! attributed, expiring waiver instead of a silent `--no-verify`.
//!
//! The load-bearing rule here is the **ceiling**. Every control declares the
//! power it actually has, and a clause can never resolve harder than its
//! mechanism can support:
//!
//! ```text
//! effective = min(clause.consequence, control.power.ceiling())
//! ```
//!
//! ADR-0015 rejected advisory symbol leases because an advisory that looks like
//! a mutex grants false safety. That rule was applied once, by judgement, to one
//! feature. The ceiling makes it mechanical and general: a clause may *declare*
//! `block`, but backed only by a hint that can be stale it resolves at `review`.

use serde::{Deserialize, Serialize};

use crate::error::{LodestarError, Result};
use crate::model::Consequence;
use crate::{Goal, GoalStatus};

/// What kind of mechanism a control is. Typed adapters, deliberately not a
/// policy DSL (SPEC-CONSTITUTION §4 keeps the evaluable surface narrow).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlKind {
    /// A deterministic pass/fail check.
    Check,
    /// A measured value compared against a fixed limit.
    Threshold,
    /// A measured value compared against a recorded baseline.
    Ratchet,
    /// A required procedure was followed.
    Procedure,
    /// A bounded semantic judgment.
    Judgment,
}

/// How much force a control can actually bring. This is a property of the
/// mechanism, not of the policy it serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnforcementPower {
    /// It physically prevented the action: a hook exited non-zero, a
    /// compare-and-swap was lost, a required CI gate went red.
    Mechanical,
    /// It proves what happened after the fact from complete, deterministic
    /// data — history inspection, a manifest diff.
    Observed,
    /// It reports a hint that may be stale, partial, or self-reported.
    Advisory,
}

impl EnforcementPower {
    /// The hardest consequence this power may support.
    ///
    /// Only a mechanism that genuinely prevented something may drive `block`.
    /// Observed and advisory mechanisms cap at `review`: after-the-fact proof
    /// and a stale hint can both be wrong about *now*, and refusing work on
    /// either is the false-safety trap ADR-0015 rejected.
    pub fn ceiling(&self) -> Consequence {
        match self {
            EnforcementPower::Mechanical => Consequence::Block,
            EnforcementPower::Observed | EnforcementPower::Advisory => Consequence::Review,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            EnforcementPower::Mechanical => "mechanical",
            EnforcementPower::Observed => "observed",
            EnforcementPower::Advisory => "advisory",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "mechanical" => Some(EnforcementPower::Mechanical),
            "observed" => Some(EnforcementPower::Observed),
            "advisory" => Some(EnforcementPower::Advisory),
            _ => None,
        }
    }
}

/// Whether a control is still in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlStatus {
    Active,
    Retired,
}

/// The outcome a control reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ObservationStatus {
    Pass,
    Fail,
    /// The control could not determine an answer. Distinct from `Pass`: absence
    /// of evidence is never evidence of conformance.
    Unknown,
}

/// A versioned mechanism bound to exactly one clause.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Control {
    pub id: String,
    pub clause_id: String,
    pub kind: ControlKind,
    pub power: EnforcementPower,
    pub version: i64,
    pub configuration: Option<String>,
    pub status: ControlStatus,
    /// Who stood this control down, and when. Standing a mechanism down
    /// weakens what its clause can enforce, so it is attributed for the same
    /// reason a waiver is: an unattributed exception is indistinguishable from
    /// a rule that was never enforced. `None` on an active control, and on
    /// every control retired before this was recorded — those retirements
    /// cannot be reconstructed, and inventing an author would be worse than
    /// admitting the gap.
    pub retired_by: Option<String>,
    pub retired_at: Option<i64>,
}

/// One reported result from a control. Never a verdict on its own: conformance
/// maps it through the clause's declared consequence (SPEC-CONSTITUTION §4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlObservation {
    pub control_id: String,
    pub clause_id: String,
    pub control_version: i64,
    pub scope: String,
    pub status: ObservationStatus,
    pub measurements: Option<String>,
    pub baseline: Option<String>,
    pub evidence_refs: Vec<String>,
    pub evaluated_at: i64,
}

/// How one observation resolved against the clause that authorises it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlResolution {
    pub control_id: String,
    pub clause_id: String,
    /// The status actually used, which may be coerced to `Unknown`.
    pub status: ObservationStatus,
    /// The hardest consequence this resolution may drive.
    pub effective: Consequence,
    pub finding: String,
}

fn min_consequence(left: Consequence, right: Consequence) -> Consequence {
    // Consequence is declared advise < review < block, so ordering is its
    // severity ordering.
    if left <= right {
        left
    } else {
        right
    }
}

/// Resolve one control observation against its clause.
///
/// `clause` is the active clause the control claims to serve, if it still
/// exists. Three refusals bound escalation, each because escalating would mean
/// asserting something the input does not support:
///
/// - an **orphan** control (no clause, or a clause that is not active) has no
///   authority to escalate — a mechanism without a rule is just a preference;
/// - a **version mismatch** means the observation came from a different control
///   than the clause bound, so it is coerced to `Unknown` rather than trusted;
/// - **`Unknown`** never escalates past `advise`, because a control that could
///   not decide has not found a breach.
///
/// Only a `Fail` from a live, version-matched control bound to an active clause
/// escalates, and even then only as far as the ceiling permits.
pub fn resolve_observation(
    clause: Option<&Goal>,
    control: &Control,
    observation: &ControlObservation,
) -> ControlResolution {
    let declared = clause
        .filter(|goal| goal.status == GoalStatus::Active)
        .map(|goal| goal.consequence.unwrap_or(Consequence::Review));
    resolve_with_declared(declared, control, observation)
}

/// The built-in control backing a `forbid_change` binding (ADR-0036).
pub const FORBID_CHANGE_CONTROL_ID: &str = "control:forbid_change";
/// Version of the built-in `forbid_change` control.
pub const FORBID_CHANGE_CONTROL_VERSION: i64 = 1;

/// The built-in control backing a `forbid_change` lock on one clause.
///
/// Its power is `mechanical` because, within the Intent Plane's own authority, a
/// `violation` genuinely refuses a state transition — the task moves to
/// `blocked` rather than `done`. It prevents; it does not merely observe.
pub fn forbid_change_control(clause_id: &str) -> Control {
    Control {
        id: FORBID_CHANGE_CONTROL_ID.to_string(),
        clause_id: clause_id.to_string(),
        kind: ControlKind::Check,
        power: EnforcementPower::Mechanical,
        version: FORBID_CHANGE_CONTROL_VERSION,
        configuration: None,
        status: ControlStatus::Active,
        retired_by: None,
        retired_at: None,
    }
}

/// A failed `forbid_change` observation for one locked node.
pub fn forbid_change_observation(
    clause_id: &str,
    node_id: &str,
    evaluated_at: i64,
) -> ControlObservation {
    ControlObservation {
        control_id: FORBID_CHANGE_CONTROL_ID.to_string(),
        clause_id: clause_id.to_string(),
        control_version: FORBID_CHANGE_CONTROL_VERSION,
        scope: node_id.to_string(),
        status: ObservationStatus::Fail,
        measurements: None,
        baseline: None,
        evidence_refs: vec![node_id.to_string()],
        evaluated_at,
    }
}

mod ratchet;

// The ratchet section moved to its own module; re-exported so every
// `crate::controls::…` path a caller already uses still resolves.
pub use ratchet::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ClauseOrigin;
    use crate::GoalKind;

    fn clause(consequence: Option<Consequence>, status: GoalStatus) -> Goal {
        Goal {
            id: "goal:protected-branch".into(),
            slug: "protected-branch".into(),
            kind: GoalKind::Invariant,
            title: "Protected branch".into(),
            statement: "A protected branch advances only by reviewed merge.".into(),
            status,
            version: 1,
            parent_id: None,
            superseded_by: None,
            reason: None,
            created_at: 1,
            constitution_version: Some("constitution:v1".into()),
            rationale: None,
            scope: Some("workflow:git.publish".into()),
            evidence_contract: Some("push target ref".into()),
            consequence,
            waivable: false,
            waiver_authority: None,
            origin: ClauseOrigin::Local,
        }
    }

    fn control(power: EnforcementPower, status: ControlStatus) -> Control {
        Control {
            id: "control:pre-push".into(),
            clause_id: "goal:protected-branch".into(),
            kind: ControlKind::Check,
            power,
            version: 2,
            configuration: None,
            status,
            retired_by: None,
            retired_at: None,
        }
    }

    fn observation(status: ObservationStatus, version: i64) -> ControlObservation {
        ControlObservation {
            control_id: "control:pre-push".into(),
            clause_id: "goal:protected-branch".into(),
            control_version: version,
            scope: "workflow:git.publish".into(),
            status,
            measurements: None,
            baseline: None,
            evidence_refs: Vec::new(),
            evaluated_at: 10,
        }
    }

    #[test]
    fn a_mechanical_control_can_drive_the_block_its_clause_declares() {
        let resolved = resolve_observation(
            Some(&clause(Some(Consequence::Block), GoalStatus::Active)),
            &control(EnforcementPower::Mechanical, ControlStatus::Active),
            &observation(ObservationStatus::Fail, 2),
        );
        assert_eq!(resolved.effective, Consequence::Block);
        assert_eq!(resolved.status, ObservationStatus::Fail);
    }

    #[test]
    fn an_advisory_control_cannot_block_however_hard_its_clause_declares() {
        // The ADR-0015 false-safety rule, made mechanical: a hint that may be
        // stale must not refuse work, even under an invariant declaring block.
        for power in [EnforcementPower::Advisory, EnforcementPower::Observed] {
            let resolved = resolve_observation(
                Some(&clause(Some(Consequence::Block), GoalStatus::Active)),
                &control(power, ControlStatus::Active),
                &observation(ObservationStatus::Fail, 2),
            );
            assert_eq!(resolved.effective, Consequence::Review, "{power:?}");
            assert!(
                resolved.finding.contains("caps this at review"),
                "{resolved:?}"
            );
        }
    }

    #[test]
    fn a_clause_declaring_less_than_the_ceiling_still_wins() {
        // The ceiling only ever lowers. A mechanical control serving a clause
        // that asks for review does not get promoted to block.
        let resolved = resolve_observation(
            Some(&clause(Some(Consequence::Review), GoalStatus::Active)),
            &control(EnforcementPower::Mechanical, ControlStatus::Active),
            &observation(ObservationStatus::Fail, 2),
        );
        assert_eq!(resolved.effective, Consequence::Review);
    }

    #[test]
    fn an_orphan_control_reports_but_cannot_escalate() {
        for clause_state in [
            None,
            Some(clause(Some(Consequence::Block), GoalStatus::Draft)),
        ] {
            let resolved = resolve_observation(
                clause_state.as_ref(),
                &control(EnforcementPower::Mechanical, ControlStatus::Active),
                &observation(ObservationStatus::Fail, 2),
            );
            assert_eq!(resolved.effective, Consequence::Advise);
            assert!(resolved.finding.contains("orphan control"), "{resolved:?}");
        }
    }

    #[test]
    fn a_version_mismatch_is_coerced_to_unknown_rather_than_trusted() {
        let resolved = resolve_observation(
            Some(&clause(Some(Consequence::Block), GoalStatus::Active)),
            &control(EnforcementPower::Mechanical, ControlStatus::Active),
            &observation(ObservationStatus::Fail, 1),
        );
        assert_eq!(resolved.status, ObservationStatus::Unknown);
        assert_eq!(resolved.effective, Consequence::Advise);
    }

    #[test]
    fn a_retired_control_is_not_current_evidence() {
        let resolved = resolve_observation(
            Some(&clause(Some(Consequence::Block), GoalStatus::Active)),
            &control(EnforcementPower::Mechanical, ControlStatus::Retired),
            &observation(ObservationStatus::Fail, 2),
        );
        assert_eq!(resolved.status, ObservationStatus::Unknown);
        assert_eq!(resolved.effective, Consequence::Advise);
    }

    #[test]
    fn unknown_never_escalates_and_is_not_conformance() {
        let resolved = resolve_observation(
            Some(&clause(Some(Consequence::Block), GoalStatus::Active)),
            &control(EnforcementPower::Mechanical, ControlStatus::Active),
            &observation(ObservationStatus::Unknown, 2),
        );
        assert_eq!(resolved.effective, Consequence::Advise);
        assert!(resolved.finding.contains("not conformance"), "{resolved:?}");
    }

    #[test]
    fn passing_satisfies_the_clause_without_escalating() {
        let resolved = resolve_observation(
            Some(&clause(Some(Consequence::Block), GoalStatus::Active)),
            &control(EnforcementPower::Mechanical, ControlStatus::Active),
            &observation(ObservationStatus::Pass, 2),
        );
        assert_eq!(resolved.status, ObservationStatus::Pass);
        assert_eq!(resolved.effective, Consequence::Advise);
    }

    #[test]
    fn an_incomplete_clause_defaults_to_review_not_block() {
        // SPEC-CONSTITUTION §10: a clause without a declared consequence is
        // review-only and can never drive a hard verdict.
        let resolved = resolve_observation(
            Some(&clause(None, GoalStatus::Active)),
            &control(EnforcementPower::Mechanical, ControlStatus::Active),
            &observation(ObservationStatus::Fail, 2),
        );
        assert_eq!(resolved.effective, Consequence::Review);
    }

    // ---- reviewed ratchets --------------------------------------------------

    fn coverage_ratchet() -> Ratchet {
        Ratchet::new(
            "control:coverage",
            "goal:protected-branch",
            "coverage.line_pct",
            RatchetDirection::NonDecreasing,
            0.5,
        )
        .unwrap()
    }

    #[test]
    fn a_ratchet_without_a_reviewed_baseline_reports_unknown_not_pass() {
        // The dangerous default. A fresh ratchet has nothing to compare
        // against, and reporting `pass` would let it certify conformance it
        // never checked — absence of evidence is not evidence of conformance.
        let observed = coverage_ratchet()
            .observe(91.0, "artifact:crates", Vec::new(), 10)
            .unwrap();
        assert_eq!(observed.status, ObservationStatus::Unknown);
        assert!(observed.baseline.is_none());
    }

    #[test]
    fn a_ratchet_fails_only_on_regression_beyond_tolerance() {
        let ratchet = coverage_ratchet()
            .with_reviewed_baseline(90.0, "monk-eee", 5)
            .unwrap();
        let status = |measured: f64| {
            ratchet
                .observe(measured, "artifact:crates", Vec::new(), 10)
                .unwrap()
                .status
        };
        assert_eq!(status(92.0), ObservationStatus::Pass, "improvement");
        assert_eq!(status(90.0), ObservationStatus::Pass, "unchanged");
        assert_eq!(status(89.5), ObservationStatus::Pass, "within tolerance");
        assert_eq!(status(89.4), ObservationStatus::Fail, "beyond tolerance");
    }

    #[test]
    fn a_non_increasing_ratchet_reads_the_other_direction() {
        let ratchet = Ratchet::new(
            "control:warnings",
            "goal:protected-branch",
            "warnings",
            RatchetDirection::NonIncreasing,
            0.0,
        )
        .unwrap()
        .with_reviewed_baseline(12.0, "monk-eee", 5)
        .unwrap();
        let status = |measured: f64| {
            ratchet
                .observe(measured, "artifact:crates", Vec::new(), 10)
                .unwrap()
                .status
        };
        assert_eq!(status(11.0), ObservationStatus::Pass);
        assert_eq!(status(13.0), ObservationStatus::Fail);
    }

    #[test]
    fn accepting_a_baseline_needs_a_reviewer_and_bumps_the_version() {
        // Attribution is the whole difference between a reviewed baseline and a
        // number the mechanism made up, and the version bump stops an
        // observation taken against the old baseline being silently re-judged
        // against a number it never saw.
        let ratchet = coverage_ratchet();
        assert_eq!(ratchet.version, 1);
        assert!(ratchet.with_reviewed_baseline(90.0, "  ", 5).is_err());

        let reviewed = ratchet.with_reviewed_baseline(90.0, "monk-eee", 5).unwrap();
        assert_eq!(reviewed.version, 2);
        assert_eq!(reviewed.baseline.as_ref().unwrap().reviewed_by, "monk-eee");
    }

    #[test]
    fn a_failed_ratchet_resolves_at_review_even_under_an_invariant() {
        // A ratchet reads a report; it stopped nothing. Its `observed` power
        // caps it at review however hard the clause it serves declares, because
        // whether a regression is acceptable is a judgement about the change.
        let ratchet = coverage_ratchet()
            .with_reviewed_baseline(90.0, "monk-eee", 5)
            .unwrap();
        let control = ratchet.control().unwrap();
        assert_eq!(control.power, EnforcementPower::Observed);
        assert_eq!(control.kind, ControlKind::Ratchet);

        let resolved = resolve_observation(
            Some(&clause(Some(Consequence::Block), GoalStatus::Active)),
            &control,
            &ratchet
                .observe(80.0, "artifact:crates", Vec::new(), 10)
                .unwrap(),
        );
        assert_eq!(resolved.status, ObservationStatus::Fail);
        assert_eq!(resolved.effective, Consequence::Review);
    }

    #[test]
    fn a_ratchet_round_trips_through_the_control_it_registers_as() {
        let ratchet = coverage_ratchet()
            .with_reviewed_baseline(90.0, "monk-eee", 5)
            .unwrap();
        let restored = Ratchet::from_control(&ratchet.control().unwrap()).unwrap();
        assert_eq!(restored, ratchet);
    }

    #[test]
    fn a_non_finite_measurement_is_refused_rather_than_compared() {
        let ratchet = coverage_ratchet()
            .with_reviewed_baseline(90.0, "monk-eee", 5)
            .unwrap();
        assert!(ratchet
            .observe(f64::NAN, "artifact:crates", Vec::new(), 10)
            .is_err());
        assert!(Ratchet::new(
            "control:x",
            "goal:y",
            "metric",
            RatchetDirection::NonDecreasing,
            -1.0
        )
        .is_err());
    }

    #[test]
    fn a_control_that_is_not_a_ratchet_does_not_parse_as_one() {
        let plain = control(EnforcementPower::Mechanical, ControlStatus::Active);
        assert!(Ratchet::from_control(&plain).is_err());
    }
}

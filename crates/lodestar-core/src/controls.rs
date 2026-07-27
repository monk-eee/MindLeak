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
    let resolution =
        |status: ObservationStatus, effective: Consequence, finding: String| ControlResolution {
            control_id: control.id.clone(),
            clause_id: control.clause_id.clone(),
            status,
            effective,
            finding,
        };

    let Some(clause) = clause.filter(|goal| goal.status == GoalStatus::Active) else {
        return resolution(
            observation.status,
            Consequence::Advise,
            format!(
                "control {} serves no active clause; an orphan control reports but cannot escalate",
                control.id
            ),
        );
    };

    if control.status == ControlStatus::Retired {
        return resolution(
            ObservationStatus::Unknown,
            Consequence::Advise,
            format!(
                "control {} is retired; its report is not current",
                control.id
            ),
        );
    }

    if observation.control_version != control.version {
        return resolution(
            ObservationStatus::Unknown,
            Consequence::Advise,
            format!(
                "control {} reported at version {} but is bound at version {}; treating as unknown",
                control.id, observation.control_version, control.version
            ),
        );
    }

    match observation.status {
        ObservationStatus::Pass => resolution(
            ObservationStatus::Pass,
            Consequence::Advise,
            format!("control {} satisfied clause {}", control.id, clause.id),
        ),
        ObservationStatus::Unknown => resolution(
            ObservationStatus::Unknown,
            Consequence::Advise,
            format!(
                "control {} could not determine an answer; absence of evidence is not conformance",
                control.id
            ),
        ),
        ObservationStatus::Fail => {
            let declared = clause.consequence.unwrap_or(Consequence::Review);
            let ceiling = control.power.ceiling();
            let effective = min_consequence(declared, ceiling);
            let finding = if effective < declared {
                format!(
                    "control {} failed clause {}; clause declares {} but a {} mechanism caps this at {}",
                    control.id,
                    clause.id,
                    declared.as_str(),
                    control.power.as_str(),
                    effective.as_str()
                )
            } else {
                format!(
                    "control {} failed clause {}; resolves {}",
                    control.id,
                    clause.id,
                    effective.as_str()
                )
            };
            resolution(ObservationStatus::Fail, effective, finding)
        }
    }
}

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
}

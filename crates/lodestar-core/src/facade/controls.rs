//! Facade surface for typed controls (ADR-0034).

use crate::controls::{
    resolve_observation, Control, ControlObservation, ControlResolution, Ratchet,
};
use crate::error::LodestarError;
use crate::{now_unix, Lodestar, Result};

impl Lodestar {
    /// Bind a versioned control to a clause. Idempotent for an unchanged
    /// control; a version never moves backwards.
    pub fn register_control(&self, control: &Control) -> Result<Control> {
        self.store.register_control(control, now_unix())
    }

    /// Every control bound to one clause.
    pub fn clause_controls(&self, clause_id: &str) -> Result<Vec<Control>> {
        self.store.controls_for_clause(clause_id)
    }

    /// Retire a control without deleting it, recording who stood it down.
    pub fn retire_control(&self, control_id: &str, retired_by: &str) -> Result<bool> {
        self.store
            .retire_control(control_id, retired_by, now_unix())
    }

    /// Resolve reported observations against the clauses that authorise them.
    ///
    /// This is where the ADR-0034 ceiling is applied: each observation is mapped
    /// through its clause's declared consequence, bounded by the power its
    /// control actually has. An observation naming a control that was never
    /// registered resolves as an orphan — it reports, and cannot escalate.
    pub fn resolve_control_observations(
        &self,
        observations: &[ControlObservation],
    ) -> Result<Vec<ControlResolution>> {
        let mut resolutions = Vec::with_capacity(observations.len());
        for observation in observations {
            let Some(control) = self.store.control(&observation.control_id)? else {
                resolutions.push(ControlResolution {
                    control_id: observation.control_id.clone(),
                    clause_id: observation.clause_id.clone(),
                    status: observation.status,
                    effective: crate::model::Consequence::Advise,
                    finding: format!(
                        "control {} is not registered; an unknown mechanism reports but cannot escalate",
                        observation.control_id
                    ),
                });
                continue;
            };
            let clause = self.store.get_goal(&control.clause_id)?;
            resolutions.push(resolve_observation(clause.as_ref(), &control, observation));
        }
        Ok(resolutions)
    }

    /// Register a reviewed ratchet as a versioned control.
    pub fn register_ratchet(&self, ratchet: &Ratchet) -> Result<Control> {
        self.store.register_control(&ratchet.control()?, now_unix())
    }

    /// Read one registered ratchet back, including the baseline in force.
    pub fn ratchet(&self, control_id: &str) -> Result<Option<Ratchet>> {
        self.store
            .control(control_id)?
            .map(|control| Ratchet::from_control(&control))
            .transpose()
    }

    /// Report a measurement to a registered ratchet and resolve it.
    ///
    /// The baseline comes from the store, never from the caller. A caller who
    /// could supply its own baseline could pick one it knows it beats, which
    /// would make the whole mechanism decorative.
    pub fn observe_ratchet(
        &self,
        control_id: &str,
        measured: f64,
        scope: &str,
        evidence_refs: Vec<String>,
    ) -> Result<ControlResolution> {
        let ratchet = self
            .ratchet(control_id)?
            .ok_or_else(|| LodestarError::NotFound(control_id.to_string()))?;
        let observation = ratchet.observe(measured, scope, evidence_refs, now_unix())?;
        self.resolve_control_observations(&[observation])?
            .pop()
            .ok_or_else(|| LodestarError::Invalid("no resolution produced".to_string()))
    }

    /// Accept a new reviewed baseline for a ratchet: attributed, justified, and
    /// it bumps the control version so stale observations resolve as `unknown`.
    pub fn accept_ratchet_baseline(
        &self,
        control_id: &str,
        value: f64,
        reviewed_by: &str,
        reason: &str,
    ) -> Result<Ratchet> {
        let ratchet = self
            .ratchet(control_id)?
            .ok_or_else(|| LodestarError::NotFound(control_id.to_string()))?;
        let reviewed = ratchet.with_reviewed_baseline(value, reviewed_by, reason, now_unix())?;
        self.store
            .register_control(&reviewed.control()?, now_unix())?;
        Ok(reviewed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::{
        ControlKind, ControlStatus, EnforcementPower, ObservationStatus, RatchetBaseline,
        RatchetDirection,
    };
    use crate::facade::test_support::engine;
    use crate::model::Consequence;
    use crate::GoalKind;

    fn observation(control_id: &str, clause_id: &str, version: i64) -> ControlObservation {
        ControlObservation {
            control_id: control_id.into(),
            clause_id: clause_id.into(),
            control_version: version,
            scope: "workflow:git.publish".into(),
            status: ObservationStatus::Fail,
            measurements: None,
            baseline: None,
            evidence_refs: Vec::new(),
            evaluated_at: 10,
        }
    }

    #[test]
    fn an_unregistered_control_reports_but_cannot_escalate() {
        // Anyone can claim a mechanism failed. Only a registered control bound
        // to a live clause carries authority to escalate.
        let e = engine();
        let resolved = e
            .resolve_control_observations(&[observation("control:ghost", "goal:anything", 1)])
            .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].effective, Consequence::Advise);
        assert!(
            resolved[0].finding.contains("not registered"),
            "{resolved:?}"
        );
    }

    #[test]
    fn a_registered_mechanical_control_resolves_through_its_clause() {
        let e = engine();
        let clause = e
            .define_goal(
                GoalKind::Invariant,
                "Protected branch",
                "A protected branch advances only by reviewed merge.",
                None,
            )
            .unwrap();
        e.register_control(&Control {
            id: "control:pre-push".into(),
            clause_id: clause.id.clone(),
            kind: ControlKind::Check,
            power: EnforcementPower::Mechanical,
            version: 1,
            configuration: None,
            status: ControlStatus::Active,
            retired_by: None,
            retired_at: None,
        })
        .unwrap();

        let resolved = e
            .resolve_control_observations(&[observation("control:pre-push", &clause.id, 1)])
            .unwrap();
        // The clause declares no consequence yet, so it is review-only
        // (SPEC-CONSTITUTION §10) even behind a mechanical control.
        assert_eq!(resolved[0].effective, Consequence::Review);
        assert_eq!(resolved[0].status, ObservationStatus::Fail);
    }

    #[test]
    fn a_retired_control_stops_carrying_current_evidence() {
        let e = engine();
        let clause = e
            .define_goal(GoalKind::Invariant, "Locked", "Do not change.", None)
            .unwrap();
        e.register_control(&Control {
            id: "control:retired".into(),
            clause_id: clause.id.clone(),
            kind: ControlKind::Check,
            power: EnforcementPower::Mechanical,
            version: 1,
            configuration: None,
            status: ControlStatus::Active,
            retired_by: None,
            retired_at: None,
        })
        .unwrap();
        assert!(e.retire_control("control:retired", "monk-eee").unwrap());

        let resolved = e
            .resolve_control_observations(&[observation("control:retired", &clause.id, 1)])
            .unwrap();
        assert_eq!(resolved[0].status, ObservationStatus::Unknown);
        assert_eq!(resolved[0].effective, Consequence::Advise);
        assert_eq!(e.clause_controls(&clause.id).unwrap().len(), 1);
    }

    // ---- reviewed ratchets --------------------------------------------------

    fn evidence_clause(e: &crate::Lodestar) -> String {
        e.define_goal(
            GoalKind::Principle,
            "Evidence before claims",
            "Do not claim success without relevant, fresh evidence.",
            None,
        )
        .unwrap()
        .id
    }

    fn coverage_ratchet(clause_id: &str) -> Ratchet {
        Ratchet::new(
            "control:coverage",
            clause_id,
            "coverage.line_pct",
            RatchetDirection::NonDecreasing,
            0.5,
        )
        .unwrap()
    }

    #[test]
    fn a_registered_ratchet_carries_its_reviewed_baseline_end_to_end() {
        let e = engine();
        let clause = evidence_clause(&e);
        e.register_ratchet(&coverage_ratchet(&clause)).unwrap();

        // Unbaselined: reports, decides nothing.
        let unknown = e
            .observe_ratchet("control:coverage", 91.0, &clause, Vec::new())
            .unwrap();
        assert_eq!(unknown.status, ObservationStatus::Unknown);
        assert_eq!(unknown.effective, Consequence::Advise);

        let reviewed = e
            .accept_ratchet_baseline(
                "control:coverage",
                90.0,
                "monk-eee",
                "the seam is real; splitting here would break cohesion",
            )
            .unwrap();
        assert_eq!(reviewed.version, 2);

        // A regression now resolves through the clause, capped at review by the
        // ratchet's observed power.
        let regressed = e
            .observe_ratchet("control:coverage", 80.0, &clause, vec!["run:1".into()])
            .unwrap();
        assert_eq!(regressed.status, ObservationStatus::Fail);
        assert_eq!(regressed.effective, Consequence::Review);

        let improved = e
            .observe_ratchet("control:coverage", 93.0, &clause, Vec::new())
            .unwrap();
        assert_eq!(improved.status, ObservationStatus::Pass);
    }

    #[test]
    fn the_stored_baseline_is_the_one_that_counts() {
        // The baseline is read from the store on every observation, so a later
        // reviewed baseline governs immediately and no caller can bring its own.
        let e = engine();
        let clause = evidence_clause(&e);
        e.register_ratchet(&coverage_ratchet(&clause)).unwrap();
        e.accept_ratchet_baseline(
            "control:coverage",
            90.0,
            "monk-eee",
            "the seam is real; splitting here would break cohesion",
        )
        .unwrap();
        assert_eq!(
            e.observe_ratchet("control:coverage", 85.0, &clause, Vec::new())
                .unwrap()
                .status,
            ObservationStatus::Fail
        );

        e.accept_ratchet_baseline(
            "control:coverage",
            84.0,
            "monk-eee",
            "the seam is real; splitting here would break cohesion",
        )
        .unwrap();
        assert_eq!(
            e.observe_ratchet("control:coverage", 85.0, &clause, Vec::new())
                .unwrap()
                .status,
            ObservationStatus::Pass
        );
        assert_eq!(e.ratchet("control:coverage").unwrap().unwrap().version, 3);
    }

    #[test]
    fn the_reason_a_baseline_was_accepted_survives_the_store() {
        // An exception nobody can read afterwards is not a recorded exception.
        // The reason has to come back out of the store, not merely be accepted
        // by the call, or the next person still cannot tell why the number moved.
        let e = engine();
        let clause = evidence_clause(&e);
        e.register_ratchet(&coverage_ratchet(&clause)).unwrap();
        e.accept_ratchet_baseline(
            "control:coverage",
            90.0,
            "monk-eee",
            "the flaky suite was deleted, so the metric measures less code",
        )
        .unwrap();

        let reloaded = e.ratchet("control:coverage").unwrap().unwrap();
        let baseline = reloaded.baseline.unwrap();
        assert!(
            baseline.reviewed_at > 0,
            "the acceptance was not timestamped"
        );
        assert_eq!(
            RatchetBaseline {
                reviewed_at: 0,
                ..baseline
            },
            RatchetBaseline {
                value: 90.0,
                reviewed_by: "monk-eee".to_string(),
                reviewed_at: 0,
                reason: Some(
                    "the flaky suite was deleted, so the metric measures less code".to_string()
                ),
            }
        );
    }

    #[test]
    fn an_observation_against_a_superseded_baseline_is_unknown_not_re_judged() {
        // The regression test for baseline laundering: evidence gathered under
        // one baseline must not be silently re-scored against another. Version
        // 1 said 80.0 was a pass; after review moved the baseline to 90.0 that
        // old report is stale, not passing.
        let e = engine();
        let clause = evidence_clause(&e);
        e.register_ratchet(&coverage_ratchet(&clause)).unwrap();
        e.accept_ratchet_baseline(
            "control:coverage",
            79.0,
            "monk-eee",
            "the seam is real; splitting here would break cohesion",
        )
        .unwrap();
        let stale = e
            .ratchet("control:coverage")
            .unwrap()
            .unwrap()
            .observe(80.0, &clause, Vec::new(), 10)
            .unwrap();
        assert_eq!(stale.status, ObservationStatus::Pass);

        e.accept_ratchet_baseline(
            "control:coverage",
            90.0,
            "monk-eee",
            "the seam is real; splitting here would break cohesion",
        )
        .unwrap();
        let resolved = e.resolve_control_observations(&[stale]).unwrap();
        assert_eq!(resolved[0].status, ObservationStatus::Unknown);
        assert_eq!(resolved[0].effective, Consequence::Advise);
    }

    #[test]
    fn a_ratchet_serving_no_clause_cannot_be_registered() {
        // §4: every ratchet must reference one constitutional clause. A
        // mechanism with no rule behind it is a preference.
        let e = engine();
        assert!(e
            .register_ratchet(&coverage_ratchet("goal:missing"))
            .is_err());
    }
}

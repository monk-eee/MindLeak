//! Facade surface for typed controls (ADR-0034).

use crate::controls::{resolve_observation, Control, ControlObservation, ControlResolution};
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

    /// Retire a control without deleting it.
    pub fn retire_control(&self, control_id: &str) -> Result<bool> {
        self.store.retire_control(control_id)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controls::{ControlKind, ControlStatus, EnforcementPower, ObservationStatus};
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
        })
        .unwrap();
        assert!(e.retire_control("control:retired").unwrap());

        let resolved = e
            .resolve_control_observations(&[observation("control:retired", &clause.id, 1)])
            .unwrap();
        assert_eq!(resolved[0].status, ObservationStatus::Unknown);
        assert_eq!(resolved[0].effective, Consequence::Advise);
        assert_eq!(e.clause_controls(&clause.id).unwrap().len(), 1);
    }
}

//! Facade surface for bounded constitutional waivers (SPEC-CONSTITUTION §9).

use crate::model::Goal;
use crate::waiver::{Waiver, WaiverRequest};
use crate::{now_unix, util, Lodestar, Result};

impl Lodestar {
    /// Grant a scoped, expiring, attributed exception to one clause.
    pub fn grant_waiver(&self, request: &WaiverRequest) -> Result<Waiver> {
        let now = now_unix();
        let id = format!(
            "waiver:{}",
            util::short_hash(&format!(
                "{}|{}|{}|{now}",
                request.clause_id, request.scope, request.approved_by
            ))
        );
        self.store.grant_waiver(&id, request, now)
    }

    /// Withdraw a waiver. Immediate for future checks, never retroactive.
    pub fn revoke_waiver(&self, waiver_id: &str, revoked_by: &str, reason: &str) -> Result<Waiver> {
        self.store
            .revoke_waiver(waiver_id, revoked_by, reason, now_unix())
    }

    /// Every waiver ever granted against one clause, including lapsed and
    /// revoked ones — how often a rule has been excepted is usually the more
    /// useful question than what is excepted right now.
    pub fn clause_waivers(&self, clause_id: &str) -> Result<Vec<Waiver>> {
        self.store.waivers_for_clause(clause_id)
    }

    /// Every waiver still capable of excusing something.
    pub fn live_waivers(&self) -> Result<Vec<Waiver>> {
        self.store.live_waivers(now_unix())
    }

    /// The waiver, if any, that excuses `target` under `clause` right now.
    ///
    /// Returns the *narrowest* match. When several waivers could apply, the one
    /// with the tightest scope is the one whose author most nearly described the
    /// situation, and reporting a broad blanket instead would overstate how much
    /// was actually reviewed.
    pub(crate) fn excusing_waiver(
        &self,
        clause: &Goal,
        target: &str,
        now: i64,
    ) -> Result<Option<Waiver>> {
        let mut matching: Vec<Waiver> = self
            .store
            .live_waivers_for_clause(&clause.id, now)?
            .into_iter()
            .filter(|waiver| waiver.applies_to(target, now))
            .collect();
        matching.sort_by(|left, right| {
            right
                .scope
                .len()
                .cmp(&left.scope.len())
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(matching.into_iter().next())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facade::test_support::engine;
    use crate::model::Consequence;
    use crate::waiver::WaiverStatus;
    use crate::GoalKind;

    fn waivable_clause(e: &Lodestar, authority: Option<&str>) -> Goal {
        let goal = e
            .define_goal(
                GoalKind::Invariant,
                "Protect the security boundary",
                "Never commit credentials.",
                None,
            )
            .unwrap();
        e.store
            .complete_clause_contract(
                &goal.id,
                "artifact:crates/**",
                "secret scan",
                Some(Consequence::Block),
                true,
                authority,
            )
            .unwrap()
    }

    fn request(clause_id: &str, scope: &str) -> WaiverRequest {
        WaiverRequest {
            clause_id: clause_id.to_string(),
            scope: scope.to_string(),
            reason: "Release blocker; remediation tracked.".into(),
            approved_by: "monk-eee".into(),
            expires_at: now_unix() + 3_600,
            remediation_task_id: None,
        }
    }

    #[test]
    fn a_granted_waiver_excuses_only_what_its_scope_reaches() {
        let e = engine();
        let clause = waivable_clause(&e, None);
        e.grant_waiver(&request(&clause.id, "artifact:crates/lodestar-core/**"))
            .unwrap();

        let now = now_unix();
        assert!(e
            .excusing_waiver(&clause, "artifact:crates/lodestar-core/src/lib.rs", now)
            .unwrap()
            .is_some());
        assert!(e
            .excusing_waiver(&clause, "artifact:crates/lodestar-mcp/src/main.rs", now)
            .unwrap()
            .is_none());
    }

    #[test]
    fn the_narrowest_matching_waiver_is_the_one_reported() {
        // Reporting a blanket when a specific exception exists would overstate
        // how much was actually reviewed.
        let e = engine();
        let clause = waivable_clause(&e, None);
        e.grant_waiver(&request(&clause.id, "artifact:crates/**"))
            .unwrap();
        e.grant_waiver(&request(
            &clause.id,
            "artifact:crates/lodestar-core/src/lib.rs",
        ))
        .unwrap();

        let found = e
            .excusing_waiver(
                &clause,
                "artifact:crates/lodestar-core/src/lib.rs",
                now_unix(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(found.scope, "artifact:crates/lodestar-core/src/lib.rs");
    }

    #[test]
    fn revocation_stops_the_exception_without_erasing_it() {
        let e = engine();
        let clause = waivable_clause(&e, None);
        let granted = e
            .grant_waiver(&request(&clause.id, "artifact:crates/**"))
            .unwrap();

        e.revoke_waiver(&granted.id, "monk-eee", "Fix landed early")
            .unwrap();
        assert!(e
            .excusing_waiver(&clause, "artifact:crates/a.rs", now_unix())
            .unwrap()
            .is_none());

        let history = e.clause_waivers(&clause.id).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, WaiverStatus::Revoked);
        assert_eq!(
            history[0].revocation_reason.as_deref(),
            Some("Fix landed early")
        );
    }

    #[test]
    fn a_clause_naming_an_authority_refuses_anyone_else() {
        let e = engine();
        let clause = waivable_clause(&e, Some("security-team"));
        assert!(e
            .grant_waiver(&request(&clause.id, "artifact:crates/**"))
            .is_err());

        let mut allowed = request(&clause.id, "artifact:crates/**");
        allowed.approved_by = "security-team".into();
        assert!(e.grant_waiver(&allowed).is_ok());
    }
}

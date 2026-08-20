use mindleak_session::SessionContext;

use crate::fleet::{
    stale_waits, wait_cycles, Divergence, FleetSession, FleetView, Presence, Staleness,
    ADVISORY_NOTE, SESSION_QUIET_GRACE_SECS, STALE_WAIT_GRACE_SECS,
};
use crate::model::TaskStatus;
use crate::{now_unix, Lodestar, Result};

impl Lodestar {
    /// Record where a session says it is working (ADR-0035 decisions 1 and 2).
    ///
    /// Stored rather than held in memory because the fleet spans processes: one
    /// spec.db is shared by every linked worktree, but each runs its own server
    /// with its own session registry. A view built on the in-process registry
    /// would report a fraction of the fleet while presenting itself as all of
    /// it — worse than reporting nothing, because it looks answered.
    pub fn declare_session_context(&self, agent_id: &str, context: &SessionContext) -> Result<()> {
        self.store
            .declare_session_context(agent_id, context, now_unix())
    }

    /// The read-only fleet view (ADR-0035 decision 4).
    ///
    /// Advisory throughout: every input is self-reported, so under the ADR-0034
    /// ceiling rule this can never do more than prompt a review. It derives only
    /// what declared data actually supports — which bases disagree, and how far
    /// behind each session said it was — and reports `unknown` for the rest
    /// rather than guessing.
    ///
    /// Waits and wait cycles (ADR-0046) are the exception to "self-reported":
    /// they are read from the ledger's own unanswered addressed questions, not
    /// declared by anyone. They stay advisory anyway, because the remedy is a
    /// human answering a question, and a view that blocked on its own
    /// observation would be a control nobody asked for.
    pub fn fleet_view(&self) -> Result<FleetView> {
        let now = now_unix();
        let live: Vec<_> = self
            .store
            .board(false)?
            .into_iter()
            .filter(|task| task.status == TaskStatus::Claimed)
            .filter_map(|task| task.owner.clone().map(|owner| (owner, task.id)))
            .collect();

        let mut sessions: Vec<FleetSession> = Vec::new();
        for (agent_id, context, declared_at) in self.store.declared_contexts()? {
            let claimed_task_ids: Vec<String> = live
                .iter()
                .filter(|(owner, _)| *owner == agent_id)
                .map(|(_, task_id)| task_id.clone())
                .collect();
            let presence = Presence::from_session(
                !claimed_task_ids.is_empty(),
                declared_at,
                now,
                SESSION_QUIET_GRACE_SECS,
            );
            sessions.push(FleetSession {
                staleness: Staleness::from_declared(context.behind),
                presence,
                agent_id,
                context,
                declared_at,
                claimed_task_ids,
            });
        }

        // A session with no declared context still holds claims, and hiding it
        // would make the view look more settled than the fleet is.
        for (agent_id, task_id) in &live {
            if sessions.iter().any(|session| session.agent_id == *agent_id) {
                continue;
            }
            match sessions
                .iter_mut()
                .find(|session| session.agent_id == *agent_id)
            {
                Some(session) => session.claimed_task_ids.push(task_id.clone()),
                None => sessions.push(FleetSession {
                    agent_id: agent_id.clone(),
                    context: SessionContext::default(),
                    declared_at: 0,
                    staleness: Staleness::Unknown,
                    // Holds a live claim by construction (it is in `live`), so
                    // it is unconditionally `Live` regardless of the placeholder
                    // `declared_at: 0` above.
                    presence: Presence::Live,
                    claimed_task_ids: vec![task_id.clone()],
                }),
            }
        }
        sessions.sort_by(|a, b| a.agent_id.cmp(&b.agent_id));

        let waits = self.store.waits()?;
        let cycles = wait_cycles(&waits);
        let stale = stale_waits(
            &waits,
            &sessions,
            &cycles,
            now_unix(),
            STALE_WAIT_GRACE_SECS,
        );
        Ok(FleetView {
            divergence: divergence_of(&sessions),
            stale_waits: stale,
            wait_cycles: cycles,
            waits,
            sessions,
            enforcement: ADVISORY_NOTE,
        })
    }
}

/// Divergence is a disagreement between *declared* bases.
///
/// Sessions that declared nothing are counted separately rather than folded in:
/// silence is not agreement, and treating it as agreement is how an advisory
/// signal starts lying.
fn divergence_of(sessions: &[FleetSession]) -> Divergence {
    let working: Vec<&FleetSession> = sessions
        .iter()
        .filter(|session| !session.claimed_task_ids.is_empty())
        .collect();
    let mut bases: Vec<String> = working
        .iter()
        .filter_map(|session| session.context.base.clone())
        .collect();
    bases.sort();
    bases.dedup();
    Divergence {
        undeclared_sessions: working
            .iter()
            .filter(|session| session.context.base.is_none())
            .count(),
        diverged: bases.len() > 1,
        bases,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::engine;
    use super::*;
    use crate::model::GoalKind;

    fn context(base: &str, behind: Option<i64>) -> SessionContext {
        SessionContext {
            branch: Some("fleet/work".to_string()),
            head_sha: Some("abc1234".to_string()),
            base: Some(base.to_string()),
            dirty: Some(false),
            behind,
        }
    }

    // ADR-0035 decision 6 / ADR-0044: an undeclared session reports unknown
    // rather than zero. "Up to date" and "never measured" are different facts,
    // and collapsing them is what makes an advisory signal untrustworthy.
    #[test]
    fn an_undeclared_session_is_unknown_not_current() {
        assert_eq!(Staleness::from_declared(None), Staleness::Unknown);
        assert_eq!(Staleness::from_declared(Some(0)), Staleness::Current);
        assert_eq!(Staleness::from_declared(Some(3)), Staleness::Behind(3));
    }

    #[test]
    fn declared_context_is_durable_and_replaced_wholesale() {
        let e = engine();
        e.declare_session_context("agent-a", &context("origin/main", Some(2)))
            .unwrap();
        let (stored, _) = e.store.session_context("agent-a").unwrap().unwrap();
        assert_eq!(stored.base.as_deref(), Some("origin/main"));
        assert_eq!(stored.behind, Some(2));

        // A later declaration that omits a field asserts it is no longer known.
        e.declare_session_context("agent-a", &SessionContext::default())
            .unwrap();
        let (cleared, _) = e.store.session_context("agent-a").unwrap().unwrap();
        assert!(!cleared.is_declared());
    }

    // Divergence is a disagreement between declared bases, and only that.
    #[test]
    fn divergence_needs_two_declared_bases_that_disagree() {
        let e = engine();
        let goal = e
            .define_goal(GoalKind::Objective, "Fleet", "coordinate", None)
            .unwrap();
        let first = e.create_task(&goal.id, "First", "done").unwrap();
        let second = e.create_task(&goal.id, "Second", "done").unwrap();
        assert!(e.claim_task(&first.id, "agent-a", 600).unwrap());
        assert!(e.claim_task(&second.id, "agent-b", 600).unwrap());

        e.declare_session_context("agent-a", &context("origin/main", Some(0)))
            .unwrap();
        e.declare_session_context("agent-b", &context("origin/main", Some(1)))
            .unwrap();
        let agreed = e.fleet_view().unwrap();
        assert!(!agreed.divergence.diverged);
        assert_eq!(agreed.divergence.bases, vec!["origin/main".to_string()]);

        e.declare_session_context("agent-b", &context("origin/release", None))
            .unwrap();
        let split = e.fleet_view().unwrap();
        assert!(split.divergence.diverged);
        assert_eq!(
            split.divergence.bases,
            vec!["origin/main".to_string(), "origin/release".to_string()]
        );
    }

    // Silence must not read as agreement: a claiming session that declared no
    // base is counted, not quietly folded into the consensus.
    #[test]
    fn an_undeclared_base_is_counted_rather_than_treated_as_agreement() {
        let e = engine();
        let goal = e
            .define_goal(GoalKind::Objective, "Fleet", "coordinate", None)
            .unwrap();
        let first = e.create_task(&goal.id, "First", "done").unwrap();
        let second = e.create_task(&goal.id, "Second", "done").unwrap();
        assert!(e.claim_task(&first.id, "agent-a", 600).unwrap());
        assert!(e.claim_task(&second.id, "agent-b", 600).unwrap());
        e.declare_session_context("agent-a", &context("origin/main", Some(0)))
            .unwrap();

        let view = e.fleet_view().unwrap();
        assert!(
            !view.divergence.diverged,
            "one declared base cannot diverge"
        );
        assert_eq!(view.divergence.undeclared_sessions, 1);

        let silent = view
            .sessions
            .iter()
            .find(|session| session.agent_id == "agent-b")
            .expect("a claiming session appears even with no declaration");
        assert_eq!(silent.staleness, Staleness::Unknown);
        assert_eq!(silent.claimed_task_ids, vec![second.id]);
    }

    // The view carries its own ceiling so a reader cannot mistake it for a gate.
    #[test]
    fn the_view_states_that_it_never_gates() {
        let e = engine();
        assert!(e.fleet_view().unwrap().enforcement.contains("advisory"));
    }

    // ADR-0046 gap closure: two agents addressing each other both sit in
    // needs_input, which the board renders as ordinary parked work. Before this
    // the fleet view showed claims and staleness but not who was waiting on
    // whom, so a pair could burn the whole seven-day parking grace doing nothing
    // while every surface read healthy. The view now derives the wait graph from
    // the ledger's own unanswered addressed questions.
    #[test]
    fn the_view_surfaces_a_wait_cycle_and_names_the_tasks_that_break_it() {
        let e = engine();
        let goal = e
            .define_goal(GoalKind::Objective, "Fleet", "coordinate", None)
            .unwrap();
        let first = e.create_task(&goal.id, "First", "done").unwrap();
        let second = e.create_task(&goal.id, "Second", "done").unwrap();
        assert!(e.claim_task(&first.id, "agent-a", 600).unwrap());
        assert!(e.claim_task(&second.id, "agent-b", 600).unwrap());

        // Healthy work: claimed, nobody waiting.
        let working = e.fleet_view().unwrap();
        assert!(working.waits.is_empty());
        assert!(working.wait_cycles.is_empty());

        // Each asks the other and parks. Both look like legitimate waits.
        assert!(e
            .ask_question(&first.id, "agent-a", "did you rename it?", Some("agent-b"))
            .unwrap());
        let one_sided = e.fleet_view().unwrap();
        assert_eq!(one_sided.waits.len(), 1);
        assert!(
            one_sided.wait_cycles.is_empty(),
            "agent-b can still answer, so a one-way wait is not a deadlock"
        );
        assert!(
            one_sided.stale_waits.is_empty(),
            "agent-b holds a live claim and the wait is fresh, so it is not stale"
        );

        assert!(e
            .ask_question(&second.id, "agent-b", "did you?", Some("agent-a"))
            .unwrap());
        let stuck = e.fleet_view().unwrap();
        assert_eq!(stuck.wait_cycles.len(), 1);
        assert_eq!(
            stuck.wait_cycles[0].agents,
            vec!["agent-a".to_string(), "agent-b".to_string()]
        );
        let mut expected = vec![first.id.clone(), second.id.clone()];
        expected.sort();
        assert_eq!(stuck.wait_cycles[0].task_ids, expected);
        assert!(
            stuck.stale_waits.is_empty(),
            "a mutual cycle is reported as a cycle, never as a stale one-way wait"
        );

        // Answering either question breaks it — the remedy the finding implies
        // must actually work, or the report is just an alarm.
        assert!(e
            .answer_question(&first.id, "yes, yesterday", "human", 600)
            .unwrap());
        let freed = e.fleet_view().unwrap();
        assert!(freed.wait_cycles.is_empty());
        assert_eq!(freed.waits.len(), 1, "agent-b is still waiting on agent-a");
    }

    // A question whose task has moved on is history, not a live wait. Counting
    // it would manufacture a stall that no longer exists.
    #[test]
    fn a_wait_ends_when_its_task_leaves_needs_input() {
        let e = engine();
        let goal = e
            .define_goal(GoalKind::Objective, "Fleet", "coordinate", None)
            .unwrap();
        let task = e.create_task(&goal.id, "First", "done").unwrap();
        assert!(e.claim_task(&task.id, "agent-a", 600).unwrap());
        assert!(e
            .ask_question(&task.id, "agent-a", "which schema?", Some("agent-b"))
            .unwrap());
        assert_eq!(e.fleet_view().unwrap().waits.len(), 1);

        assert!(e.answer_question(&task.id, "v2", "agent-b", 600).unwrap());
        assert!(e.fleet_view().unwrap().waits.is_empty());
    }
}

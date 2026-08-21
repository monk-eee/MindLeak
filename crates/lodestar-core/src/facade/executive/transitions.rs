//! Task lifecycle transitions: block, reopen, abandon, human-accept, and the
//! owner-guarded pause/resume pair.

use crate::{now_unix, Lodestar, LodestarError, Result};

impl Lodestar {
    /// Mark a task blocked, optionally on one validated predecessor. A non-empty
    /// `reason` lands in the task's durable thread (ADR-0046) so the agent that
    /// held the work can read why it was taken away.
    pub fn block_task(
        &self,
        id: &str,
        blocked_by: Option<String>,
        reason: Option<&str>,
        actor: &str,
    ) -> Result<bool> {
        self.store
            .block_task(id, blocked_by, reason, actor, now_unix())
    }

    /// Return a stranded task (in review, or manually blocked with no live
    /// predecessor gate) to `open` so an agent can claim it again. Refuses to
    /// bypass a handoff dependency, disturb an active claim, or revive terminal
    /// work.
    pub fn reopen_task(&self, id: &str) -> Result<bool> {
        self.store.reopen_task(id, now_unix())
    }

    /// Permanently retire a nonterminal task to `abandoned` (terminal). The
    /// deliberate "do not do this work" verb, distinct from `reopen_task`
    /// (recover) and `reset` (wipe). Refuses to disturb live or parked ownership,
    /// but permits an expired claim to be retired. A task that recorded a branch
    /// refuses unless `acknowledge_branch` is set, so branched work that may have
    /// shipped is not retired without the caller confirming they checked.
    pub fn abandon_task(&self, id: &str, acknowledge_branch: bool) -> Result<bool> {
        self.store.abandon_task(id, acknowledge_branch, now_unix())
    }

    /// Accept an `in_review` task to `done` under a reviewer label — the
    /// task-level mirror of
    /// `accept_design` (ADR-0009 close-out). A task lands in `in_review` when
    /// conformance returns `drift`/`needs_human`: the work is plausibly done but
    /// a reviewer must judge it. `resolve_task` records that judgement and moves
    /// the task to `done` with no code-conformance re-run; the terminal
    /// transition opens any blocked successor.
    ///
    /// The label is an attributable declaration, not authentication
    /// (ADR-0071). Lodestar has no human identity provider and proves only that
    /// the non-empty label differs from the agent id in the evidence under
    /// review. `resolved_by` records exactly that label and nothing stronger.
    pub fn resolve_task(&self, id: &str, reviewer_label: &str) -> Result<bool> {
        let reviewer_label = reviewer_label.trim();
        if reviewer_label.is_empty() {
            return Err(LodestarError::Invalid(
                "a non-empty reviewer label is required; it is recorded for attribution, \
                 not authenticated as a human identity"
                    .to_string(),
            ));
        }
        // A same-string self-review remains forbidden. This is a guard against
        // the reviewed agent naming itself, not proof that any other label names
        // a person: the local stdio plane has no identity source to consult.
        if let Some(worker) = self.review_worker(id)? {
            if worker == reviewer_label {
                return Err(LodestarError::Invalid(
                    "the reviewed agent may not resolve its own in_review task; provide a \
                     distinct reviewer label (attributed, not authenticated)"
                        .to_string(),
                ));
            }
        }
        self.store.resolve_in_review(id, reviewer_label, now_unix())
    }

    /// The agent whose most recent conformance check put a task in review, read
    /// from the durable evidence bundle. Backs the `resolve_task`
    /// human-in-the-loop guard; `None` when the task has no conformance history.
    fn review_worker(&self, id: &str) -> Result<Option<String>> {
        let history = self.store.conformance_history(id)?;
        Ok(history.last().and_then(|record| {
            serde_json::from_str::<serde_json::Value>(&record.evidence)
                .ok()
                .and_then(|bundle| {
                    bundle
                        .get("agent_id")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                })
        }))
    }

    /// Deliberately suspend a claimed task (ADR-0020): owner-guarded move to
    /// `paused`, keeping the owner + evidence window but clearing the live lease.
    /// A non-empty `reason` lands in the durable thread (ADR-0046).
    pub fn pause_task(&self, id: &str, agent: &str, reason: Option<&str>) -> Result<bool> {
        let agent = self.resolve_agent(agent)?;
        self.store.pause_task(id, agent, reason, now_unix())
    }

    /// Resume a paused task under the same owner with a fresh lease (ADR-0020).
    pub fn resume_task(&self, id: &str, agent: &str, lease_secs: i64) -> Result<bool> {
        let agent = self.resolve_agent(agent)?;
        self.store.resume_task(id, agent, lease_secs, now_unix())
    }
}

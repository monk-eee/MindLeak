//! Task creation, duplicate detection, and goal decomposition.

use serde::{Deserialize, Serialize};

use crate::error::ModelFailureReason;
use crate::llm::ModelCallProvenance;
use crate::{now_unix, Lodestar, LodestarError, Result, Task};

/// One task a decomposition resolved to, with additive model-call provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecomposedTask {
    #[serde(flatten)]
    pub task: Task,
    pub model_call: ModelCallProvenance,
    /// Whether this draft resolved to work that already existed rather than to
    /// a task this run created. Reported because the two are indistinguishable
    /// from the task alone, and a caller told it created work it did not create
    /// will go looking for a second copy.
    pub reused: bool,
}

impl std::ops::Deref for DecomposedTask {
    type Target = Task;

    fn deref(&self) -> &Self::Target {
        &self.task
    }
}

impl Lodestar {
    pub fn create_task(&self, goal_id: &str, title: &str, acceptance: &str) -> Result<Task> {
        self.store
            .create_task(goal_id, title, acceptance, None, now_unix())
    }

    pub fn create_task_after(
        &self,
        goal_id: &str,
        title: &str,
        acceptance: &str,
        blocked_by: Option<String>,
    ) -> Result<Task> {
        self.create_task_covering(goal_id, title, acceptance, blocked_by, &[])
    }

    /// Create a task that declares, up front, the additional goals it serves
    /// (ADR-0041). Declared before the work it is a prediction the evidence can
    /// contradict; the same declaration after a finding has been raised would be
    /// a rationalisation, which is why [`Lodestar::declare_coverage`] closes at
    /// the first verdict rather than at creation.
    pub fn create_task_covering(
        &self,
        goal_id: &str,
        title: &str,
        acceptance: &str,
        blocked_by: Option<String>,
        also_serves: &[String],
    ) -> Result<Task> {
        self.store.create_task_covering(
            goal_id,
            title,
            acceptance,
            None,
            blocked_by,
            also_serves,
            now_unix(),
        )
    }

    /// Live work under this goal already carrying this exact title.
    ///
    /// The same lookup the generators dedupe on, exposed so a caller that is
    /// allowed to create a duplicate can still be told it is about to. Reports;
    /// never refuses (ADR-0015).
    pub fn live_task_titled(&self, goal_id: &str, title: &str) -> Result<Option<Task>> {
        self.store.live_task_titled(goal_id, title)
    }

    /// Live work carrying this exact title under some other goal.
    ///
    /// The same-goal rule cannot see the shape that actually filled this board:
    /// a generator run once per active goal, producing one identically titled
    /// task under each in the same second. Reports; never refuses.
    pub fn live_tasks_titled_elsewhere(&self, goal_id: &str, title: &str) -> Result<Vec<Task>> {
        self.store.live_tasks_titled_elsewhere(goal_id, title)
    }

    /// Declare further goals the held claim serves, before conformance speaks.
    ///
    /// Goals bind to files, so the governing set is usually learned while
    /// working rather than predicted at creation. This lets the agent say so
    /// while its declaration is still a prediction; once any conformance record
    /// exists for the task it is refused, because coverage widened after a
    /// finding is an excuse for that finding. Unions with what was already
    /// declared, so naming what you just learned never drops what you knew.
    pub fn declare_coverage(
        &self,
        task_id: &str,
        agent: &str,
        also_serves: &[String],
    ) -> Result<Vec<String>> {
        self.store
            .declare_coverage(task_id, agent, also_serves, now_unix())
    }

    /// The additional goals a task declared it serves (ADR-0041).
    pub fn task_goal_coverage(&self, task_id: &str) -> Result<Vec<String>> {
        self.store.goal_coverage(task_id)
    }

    /// Reconnect the caller's own live claim from a superseded clause onto its
    /// active same-slug successor, at their own request (ADR-0109). Refused for
    /// any task the caller does not currently hold with an unexpired lease, or
    /// whose current clause has no unique active successor to move onto.
    pub fn reconnect_claim_clause(&self, task_id: &str, agent: &str) -> Result<String> {
        let agent = self.resolve_agent(agent)?;
        self.store
            .reconnect_claim_clause(task_id, agent, now_unix())
    }

    /// Break a goal into tasks. Uses the local model when reachable, else a
    /// deterministic single-task fallback so the plane works with no LLM.
    ///
    /// Idempotent per `(goal, title)` over live work: a draft whose exact title
    /// already names a non-terminal task under this goal resolves to that task
    /// instead of a second copy of it. Decomposition is a generator, and a
    /// generator that runs twice must not produce the work twice — the second
    /// copy adds no work, only two agents each holding a task they believe is
    /// the only one. Measured here: repeated runs left three or four identical
    /// `Implement: ADR-NNNN` seeds per ADR, and two sessions independently
    /// claimed and built ADR-0090. The lookup is shared with design
    /// materialization so the two generators cannot drift apart on it.
    ///
    /// Deliberately not a rule on [`create_task`](Self::create_task), where
    /// ADR-0015 holds that a second task against one goal is often right and a
    /// gate would be wrong more often than right. The difference is authorship:
    /// a person asking for another task has decided they want one; a generator
    /// re-emitting a draft it already emitted has decided nothing.
    ///
    /// Terminal work does not suppress a draft. A `done` or `abandoned` task is
    /// history, nothing dispatches it to an agent, and re-decomposing after
    /// retiring the old breakdown is a deliberate act that should produce work.
    pub fn decompose_goal(&self, goal_id: &str) -> Result<Vec<DecomposedTask>> {
        let goal = self
            .store
            .get_goal(goal_id)?
            .ok_or_else(|| LodestarError::NotFound(goal_id.to_string()))?;
        // Only objectives decompose into claimable work. Constraints and
        // invariants are enforced continuously by conformance, so decomposing
        // them only yields tasks that restate the rule and can never accrue
        // completion evidence — the noise that buries real work in next_task.
        if goal.kind.is_normative() {
            return Err(LodestarError::Invalid(format!(
                "goal {goal_id} is a {} enforced by conformance, not completed as \
                 discrete tasks; decomposing it only produces non-actionable \
                 restatements. Define an objective goal for the work that \
                 satisfies it instead.",
                goal.kind.as_str()
            )));
        }
        let now = now_unix();
        let (drafts, model_call) =
            self.decompose_drafts_with_provenance(&goal.title, &goal.statement);
        let mut out = Vec::new();
        for (title, acceptance) in &drafts {
            // One lookup, shared with design materialization, so the two
            // generators cannot answer "already live work" differently. Asked
            // per draft rather than from a snapshot, which also catches a model
            // that emits one title twice in a single batch.
            let (task, reused) = match self.store.live_task_titled(goal_id, title)? {
                Some(found) => (found, true),
                None => (
                    self.store
                        .create_task(goal_id, title, acceptance, None, now)?,
                    false,
                ),
            };
            out.push(DecomposedTask {
                task,
                model_call,
                reused,
            });
        }
        Ok(out)
    }

    /// Model-assisted task drafts `(title, acceptance)` for a title/statement,
    /// with a deterministic single-task fallback when no model is reachable.
    /// Shared by `decompose_goal` and read-only design materialization planning
    /// (ADR-0023) so both breakdowns behave identically.
    pub(crate) fn decompose_drafts_with_provenance(
        &self,
        title: &str,
        statement: &str,
    ) -> (Vec<(String, String)>, ModelCallProvenance) {
        let started = std::time::Instant::now();
        let result = self.llm.decompose(title, statement);
        let ok = matches!(&result, Ok(drafts) if !drafts.is_empty());
        self.record_model_call(
            "decompose",
            ok,
            started.elapsed().as_millis() as i64,
            Some(&self.llm.model),
            self.llm.last_usage(),
        );
        match result {
            Ok(drafts) if !drafts.is_empty() => (
                drafts
                    .into_iter()
                    .map(|draft| (draft.title, draft.acceptance))
                    .collect(),
                ModelCallProvenance::model(),
            ),
            Ok(_) => (
                vec![(format!("Implement: {title}"), statement.to_string())],
                ModelCallProvenance::fallback(ModelFailureReason::BadJson),
            ),
            Err(error) => (
                vec![(format!("Implement: {title}"), statement.to_string())],
                ModelCallProvenance::fallback(
                    error
                        .model_failure()
                        .map(|failure| failure.reason)
                        .unwrap_or(ModelFailureReason::BadJson),
                ),
            ),
        }
    }
}

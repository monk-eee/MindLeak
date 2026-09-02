//! Durable dialogue: pre-flight question drafts, addressing a peer or a
//! human, and reading the append-only thread.

use crate::dialogue::{self, DraftedBy, QuestionDraft};
use crate::error::ModelFailureReason;
use crate::llm::{ModelCallProvenance, ModelCallSource};
use crate::{
    now_unix, ClaimOverlap, FederatedClaimOutcome, HumanQuestion, Lodestar, LodestarError, Result,
    TaskQa,
};

impl Lodestar {
    /// Propose the questions this task's owner could put to peers whose live
    /// claims collide with it (ADR-0055).
    ///
    /// Read-only and evidence-free: it records nothing, parks nothing, and
    /// addresses nothing. `ask_question` remains the only thing that changes
    /// task state, so a draft nobody sends leaves no trace — which is what lets
    /// this be generous with suggestions without polluting the durable thread.
    ///
    /// The collision is found deterministically from declared scope. Only the
    /// *phrasing* is model-assisted, and it falls back to the template when no
    /// model is reachable, so the capability never depends on one.
    pub fn draft_questions(&self, task_id: &str) -> Result<Vec<QuestionDraft>> {
        let task = self
            .store
            .get_task(task_id)?
            .ok_or_else(|| LodestarError::NotFound(task_id.to_string()))?;
        let scope = self.store.task_scope(task_id)?;
        // The asking side is this task's owner, so the branch each collision is
        // classified against is that agent's declared one, not the caller's.
        let overlaps = self.store.check_claim_overlap(
            &scope,
            Some(task_id),
            task.owner.as_deref(),
            now_unix(),
        )?;
        let mut drafts = Vec::new();
        for overlap in overlaps.claims {
            // An agent may not address a question to itself (ADR-0046 clause 6):
            // it would park the task waiting on the only agent that cannot act
            // while it is parked. Two of one agent's own tasks colliding is
            // ordinary, so this is a skip and not an error.
            if task.owner.as_deref() == Some(overlap.owner.as_str()) {
                continue;
            }
            let their_title = self
                .store
                .get_task(&overlap.task_id)?
                .map(|peer| peer.title)
                .unwrap_or_default();
            let (question, drafted_by, model_call) =
                self.phrase_question(&task.title, &their_title, &overlap);
            drafts.push(QuestionDraft {
                audience: overlap.owner,
                their_task_id: overlap.task_id,
                their_title,
                matching_paths: overlap.matching_paths,
                matching_symbols: overlap.matching_symbols,
                question,
                drafted_by,
                model_call,
            });
        }
        Ok(drafts)
    }

    /// The question text, model-phrased when one is reachable and templated
    /// otherwise. The provenance travels with it so a drafted sentence is never
    /// mistaken for a recorded fact.
    fn phrase_question(
        &self,
        my_title: &str,
        their_title: &str,
        overlap: &ClaimOverlap,
    ) -> (String, DraftedBy, ModelCallProvenance) {
        let template = dialogue::template_question(my_title, overlap);
        let shared: Vec<&str> = overlap
            .matching_paths
            .iter()
            .chain(overlap.matching_symbols.iter())
            .map(String::as_str)
            .collect();
        if shared.is_empty() {
            return (
                template,
                DraftedBy::Template,
                ModelCallProvenance {
                    source: ModelCallSource::Fallback,
                    fallback_reason: None,
                },
            );
        }
        let started = std::time::Instant::now();
        let result = self
            .llm
            .draft_question(my_title, their_title, &shared.join(", "));
        self.record_model_call(
            "draft_question",
            result.is_ok(),
            started.elapsed().as_millis() as i64,
            Some(&self.llm.model),
            self.llm.last_usage(),
        );
        match result {
            Ok(question) => (question, DraftedBy::Model, ModelCallProvenance::model()),
            Err(error) => (
                template,
                DraftedBy::Template,
                ModelCallProvenance::fallback(
                    error
                        .model_failure()
                        .map(|failure| failure.reason)
                        .unwrap_or(ModelFailureReason::BadJson),
                ),
            ),
        }
    }

    /// Park a claimed task with a durable question (ADR-0020): owner-guarded
    /// move to `needs_input` that keeps the owner + evidence window but clears
    /// the live lease. `audience` addresses it at a peer agent rather than a
    /// human (ADR-0046). Answer it with `answer_question`.
    ///
    /// Routes through a federated repository's Ackplane claim CAS when
    /// [`with_federated_claim_authority`](Self::with_federated_claim_authority)
    /// was called (ADR-0096 clause completion): Ackplane is the sole
    /// authority over the claim-state transition, so no local CAS decision
    /// runs before or after the remote request. The question text itself
    /// stays local either way (ADR-0020's task_qa thread) -- Ackplane never
    /// becomes a mode of the local plane's own dialogue.
    pub fn ask_question(
        &self,
        id: &str,
        agent: &str,
        question: &str,
        audience: Option<&str>,
    ) -> Result<bool> {
        let agent = self.resolve_agent(agent)?;
        // Mirrors `LodestarStore::ask_question`'s own normalization: blank
        // addresses a human, never nothing. Duplicated (not called through)
        // because the federated path below must apply this rule *before*
        // asking Ackplane to park anything -- `ParkClaim`'s wire contract
        // carries no audience field at all (the question stays local), so
        // Ackplane has no way to refuse a self-addressed question itself.
        let audience = audience
            .map(str::trim)
            .filter(|addressee| !addressee.is_empty());
        if audience == Some(agent) {
            return Err(LodestarError::Invalid(format!(
                "task {id}: an agent cannot address a question to itself"
            )));
        }
        if let Some(authority) = &self.federated_claim_authority {
            let parked = authority.park(id, agent)?;
            if parked {
                self.store
                    .apply_federated_park(id, agent, question, audience, now_unix())?;
            }
            return Ok(parked);
        }
        self.store
            .ask_question(id, agent, question, audience, now_unix())
    }

    /// Unanswered questions addressed to one agent, oldest first (ADR-0046). A
    /// read over the durable thread, not a queue: nothing is delivered or
    /// consumed, so reading can never lose a question.
    pub fn pending_questions(&self, agent: &str) -> Result<Vec<TaskQa>> {
        let agent = self.resolve_agent(agent)?;
        self.store.pending_questions(agent)
    }

    /// Everything currently waiting on a person, oldest first (ADR-0046).
    ///
    /// The human counterpart of `pending_questions`, and necessarily a separate
    /// query: a human has no agent id, so `audience IS NULL` is the addressing
    /// and matching on an id can never find one. Read-only and evidence-free —
    /// it records nothing and changes no task state, and reading a question
    /// cannot consume it.
    pub fn questions_for_a_human(&self) -> Result<Vec<HumanQuestion>> {
        self.store.questions_for_a_human(now_unix())
    }

    /// Answer a `needs_input` task (ADR-0020): records the durable answer and
    /// returns the task to `claimed` under the same owner with a fresh lease.
    ///
    /// Routes through a federated repository's Ackplane claim CAS when
    /// [`with_federated_claim_authority`](Self::with_federated_claim_authority)
    /// was called (ADR-0096 clause completion). Unlike the local path,
    /// `author` need not be the task's owner -- ADR-0046's whole point is a
    /// peer answering a question addressed to it, not to itself -- so this
    /// reads the currently-parked owner from the local cache and asks
    /// Ackplane to grant *that* owner the fresh lease, regardless of which
    /// agent supplied the answer text.
    pub fn answer_question(
        &self,
        id: &str,
        answer: &str,
        author: &str,
        lease_secs: i64,
    ) -> Result<bool> {
        if let Some(authority) = &self.federated_claim_authority {
            let task = self
                .store
                .get_task(id)?
                .ok_or_else(|| LodestarError::NotFound(id.to_string()))?;
            let Some(owner) = task.owner else {
                return Ok(false);
            };
            return match authority.answer(id, &owner, lease_secs)? {
                FederatedClaimOutcome::Granted(grant) => {
                    self.store
                        .apply_federated_answer(id, author, answer, &grant, now_unix())?;
                    Ok(true)
                }
                FederatedClaimOutcome::Rejected { .. } => Ok(false),
            };
        }
        self.store
            .answer_question(id, answer, author, lease_secs, now_unix())
    }

    /// The durable, append-only dialogue thread for a task (ADR-0020, ADR-0046).
    pub fn task_qa(&self, task_id: &str) -> Result<Vec<TaskQa>> {
        self.store.task_qa(task_id)
    }
}

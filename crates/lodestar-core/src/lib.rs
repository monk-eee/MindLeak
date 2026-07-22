//! Lodestar — the Intent Plane for MindLeak.
//!
//! The durable, authoritative counterpart to the decaying memory graph: a
//! versioned constitution (goals/constraints/invariants), an executive task
//! ledger with atomic claim/lease coordination for parallel local agents, a
//! conformance check that flags drift/violations, and consolidated learned
//! knowledge that is durable-but-revalidated (ADR-0004, ADR-0005, SPEC-INTENT).

pub mod db;
pub mod decay;
pub mod error;
pub mod llm;
pub mod model;
pub mod store;
mod util;

use std::time::{SystemTime, UNIX_EPOCH};

pub use error::{LodestarError, Result};
pub use model::{
    CodeBindingMode, ConformanceEvidence, ConformanceResult, EvidenceProvenance, Goal, GoalKind,
    GoalStatus, Knowledge, Task, TaskStatus, Verdict,
};
pub use store::{LodestarStore, Stats};

use llm::LlmClient;
use store::ConformanceAudit;

/// Gating thresholds for consolidation (ADR-0005: don't launder coincidence).
const MIN_EVIDENCE_COUNT: usize = 3;
const MIN_EVIDENCE_SPAN_SECS: i64 = 3 * 24 * 3600; // proven across days, not one session
const MAX_EVIDENCE_EVENTS: usize = 200;
const MAX_EVIDENCE_PROVENANCE: usize = 1_000;
const MAX_EVIDENCE_SUMMARY_BYTES: usize = 4_096;

/// Current unix time in whole seconds.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// High-level facade over the Intent Plane store and the optional LLM.
pub struct Lodestar {
    store: LodestarStore,
    llm: LlmClient,
    agent: Option<String>,
}

impl Lodestar {
    pub fn open(path: &str) -> Result<Self> {
        Ok(Lodestar {
            store: LodestarStore::new(db::open(path)?),
            llm: LlmClient::default(),
            agent: None,
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        Ok(Lodestar {
            store: LodestarStore::new(db::open_in_memory()?),
            llm: LlmClient::default(),
            agent: None,
        })
    }

    /// Override the LLM client (dependency injection; used by tests to force the
    /// deterministic no-model fallback regardless of any local server).
    pub fn with_llm(mut self, llm: LlmClient) -> Self {
        self.llm = llm;
        self
    }

    pub fn with_agent(mut self, agent: Option<String>) -> Self {
        self.agent = agent.filter(|value| !value.trim().is_empty());
        self
    }

    pub fn store(&self) -> &LodestarStore {
        &self.store
    }

    // ---- constitution ------------------------------------------------------

    pub fn define_goal(
        &self,
        kind: GoalKind,
        title: &str,
        statement: &str,
        parent: Option<String>,
    ) -> Result<Goal> {
        self.store
            .define_goal(kind, title, statement, parent, now_unix())
    }

    pub fn supersede_goal(&self, old_id: &str, new_statement: &str, reason: &str) -> Result<Goal> {
        self.store
            .supersede_goal(old_id, new_statement, reason, now_unix())
    }

    /// The authoritative set an agent reads before acting.
    pub fn get_constitution(&self) -> Result<Vec<Goal>> {
        self.store.goals_by_status(GoalStatus::Active)
    }

    pub fn link_goal_to_code(
        &self,
        goal_id: &str,
        node_ids: &[String],
        mode: CodeBindingMode,
    ) -> Result<usize> {
        if !self.store.goal_exists(goal_id)? {
            return Err(LodestarError::NotFound(goal_id.to_string()));
        }
        let goal = self
            .store
            .get_goal(goal_id)?
            .ok_or_else(|| LodestarError::NotFound(goal_id.to_string()))?;
        if mode == CodeBindingMode::ForbidChange && !goal.kind.is_normative() {
            return Err(LodestarError::Invalid(
                "forbid_change is valid only for constraints and invariants".to_string(),
            ));
        }
        self.store.link_goal_to_code(goal_id, node_ids, mode)
    }

    /// Render the active constitution as committed-friendly markdown; optionally
    /// write it to `path`.
    pub fn export_constitution(&self, path: Option<&str>) -> Result<String> {
        let goals = self.get_constitution()?;
        let markdown = render_constitution(&goals);
        if let Some(p) = path {
            if let Some(parent) = std::path::Path::new(p).parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(p, &markdown)?;
        }
        Ok(markdown)
    }

    // ---- executive ---------------------------------------------------------

    pub fn create_task(&self, goal_id: &str, title: &str, acceptance: &str) -> Result<Task> {
        self.store
            .create_task(goal_id, title, acceptance, None, now_unix())
    }

    /// Break a goal into tasks. Uses the local model when reachable, else a
    /// deterministic single-task fallback so the plane works with no LLM.
    pub fn decompose_goal(&self, goal_id: &str) -> Result<Vec<Task>> {
        let goal = self
            .store
            .get_goal(goal_id)?
            .ok_or_else(|| LodestarError::NotFound(goal_id.to_string()))?;
        let now = now_unix();
        let drafts = match self.llm.decompose(&goal.title, &goal.statement) {
            Ok(d) if !d.is_empty() => d,
            _ => vec![llm::TaskDraft {
                title: format!("Implement: {}", goal.title),
                acceptance: goal.statement.clone(),
            }],
        };
        let mut out = Vec::new();
        for d in drafts {
            out.push(
                self.store
                    .create_task(goal_id, &d.title, &d.acceptance, None, now)?,
            );
        }
        Ok(out)
    }

    pub fn next_task(&self) -> Result<Option<Task>> {
        self.store.next_task(now_unix())
    }

    pub fn claim_task(&self, id: &str, agent: &str, lease_secs: i64) -> Result<bool> {
        let agent = self.resolve_agent(agent)?;
        self.store.claim_task(id, agent, lease_secs, now_unix())
    }

    pub fn renew_lease(&self, id: &str, agent: &str, lease_secs: i64) -> Result<bool> {
        let agent = self.resolve_agent(agent)?;
        self.store.renew_lease(id, agent, lease_secs, now_unix())
    }

    pub fn release_task(&self, id: &str, agent: &str) -> Result<bool> {
        let agent = self.resolve_agent(agent)?;
        self.store.release_task(id, agent, now_unix())
    }

    pub fn block_task(&self, id: &str, blocked_by: Option<String>) -> Result<bool> {
        self.store
            .force_status(id, TaskStatus::Blocked, blocked_by, now_unix())
    }

    pub fn board(&self) -> Result<Vec<Task>> {
        self.store.board()
    }

    /// Complete a task only when claim-bounded evidence conforms (ADR-0009).
    pub fn complete_task(
        &self,
        id: &str,
        agent: &str,
        evidence: &ConformanceEvidence,
    ) -> Result<(bool, ConformanceResult)> {
        let now = now_unix();
        let agent = self.resolve_agent(agent)?;
        let task = self
            .store
            .get_task(id)?
            .ok_or_else(|| LodestarError::NotFound(id.to_string()))?;
        self.validate_claim_evidence(&task, agent, evidence, now)?;
        let conformance = self.evaluate_conformance(evidence, Some(&task))?;
        let target_status = match conformance.verdict {
            Verdict::Aligned => TaskStatus::Done,
            Verdict::Violation => TaskStatus::Blocked,
            Verdict::Drift | Verdict::NeedsHuman => TaskStatus::InReview,
        };
        let serialized = serde_json::to_string(evidence)?;
        let findings = conformance.findings.join("; ");
        if !self.store.record_conformance_and_transition(
            id,
            agent,
            ConformanceAudit {
                evidence_schema_version: evidence.schema_version,
                evidence: &serialized,
                verdict: conformance.verdict,
                findings: &findings,
            },
            target_status,
            now,
        )? {
            return Err(LodestarError::Invalid(format!(
                "task {id} claim changed before conformance could be recorded"
            )));
        }
        Ok((conformance.verdict == Verdict::Aligned, conformance))
    }

    // ---- conformance -------------------------------------------------------

    /// Check evidence without changing task state; uses the completion evaluator.
    pub fn check_conformance(
        &self,
        evidence: &ConformanceEvidence,
        task_id: Option<&str>,
    ) -> Result<ConformanceResult> {
        let resolved_task_id = match (task_id, evidence.task_id.as_deref()) {
            (Some(left), Some(right)) if left != right => {
                return Err(LodestarError::Invalid(
                    "evidence task_id does not match requested task".to_string(),
                ));
            }
            (Some(id), _) | (_, Some(id)) => Some(id),
            (None, None) => None,
        };
        let task = match resolved_task_id {
            Some(id) => Some(
                self.store
                    .get_task(id)?
                    .ok_or_else(|| LodestarError::NotFound(id.to_string()))?,
            ),
            None => None,
        };
        if let Some(task) = task.as_ref() {
            let agent = self.resolve_agent(&evidence.agent_id)?;
            self.validate_claim_evidence(task, agent, evidence, now_unix())?;
        } else {
            self.validate_evidence_shape(evidence)?;
        }
        let conformance = self.evaluate_conformance(evidence, task.as_ref())?;
        let serialized = serde_json::to_string(evidence)?;
        let findings = conformance.findings.join("; ");
        self.store.record_conformance(
            resolved_task_id,
            ConformanceAudit {
                evidence_schema_version: evidence.schema_version,
                evidence: &serialized,
                verdict: conformance.verdict,
                findings: &findings,
            },
            now_unix(),
        )?;
        Ok(conformance)
    }

    fn evaluate_conformance(
        &self,
        evidence: &ConformanceEvidence,
        task: Option<&Task>,
    ) -> Result<ConformanceResult> {
        let mut findings = Vec::new();
        if evidence.changed_node_ids.is_empty() || evidence.provenance.is_empty() {
            findings.push("evidence contains no provenance-bearing mutation".to_string());
            return Ok(ConformanceResult {
                verdict: Verdict::NeedsHuman,
                findings,
            });
        }

        let mut touched_task_goal = false;
        let mut wrong_goals = Vec::new();
        for node in &evidence.changed_node_ids {
            for binding in self.store.active_bindings_for_node(node)? {
                if binding.mode == CodeBindingMode::ForbidChange {
                    findings.push(format!("{} forbids changes to {node}", binding.goal.id));
                    return Ok(ConformanceResult {
                        verdict: Verdict::Violation,
                        findings,
                    });
                }
                match task {
                    Some(task) if binding.goal.id == task.goal_id => touched_task_goal = true,
                    Some(_) => wrong_goals.push(binding.goal.id),
                    None => wrong_goals.push(binding.goal.id),
                }
            }
        }

        if !wrong_goals.is_empty() {
            wrong_goals.sort();
            wrong_goals.dedup();
            findings.push(format!(
                "governed code changed without a covering task: {}",
                wrong_goals.join(", ")
            ));
            return Ok(ConformanceResult {
                verdict: Verdict::Drift,
                findings,
            });
        }

        let Some(task) = task else {
            findings.push("no governed code touched".to_string());
            return Ok(ConformanceResult {
                verdict: Verdict::Aligned,
                findings,
            });
        };
        if !touched_task_goal {
            findings.push("evidence does not touch code bound to the task goal".to_string());
            return Ok(ConformanceResult {
                verdict: Verdict::NeedsHuman,
                findings,
            });
        }

        let goal = self
            .store
            .get_goal(&task.goal_id)?
            .ok_or_else(|| LodestarError::NotFound(task.goal_id.clone()))?;
        if goal.kind.is_normative() {
            match self.llm.judge(&goal.statement, &evidence.summary) {
                Ok((verdict, rationale)) if verdict == "aligned" => {
                    findings.push(format!("semantic check aligned: {rationale}"));
                }
                Ok((verdict, rationale)) if verdict == "violation" => {
                    findings.push(format!("semantic check found a violation: {rationale}"));
                    return Ok(ConformanceResult {
                        verdict: Verdict::Violation,
                        findings,
                    });
                }
                Ok((_, rationale)) => {
                    findings.push(format!("semantic check needs human review: {rationale}"));
                    return Ok(ConformanceResult {
                        verdict: Verdict::NeedsHuman,
                        findings,
                    });
                }
                Err(_) => {
                    findings.push("semantic check unavailable".to_string());
                    return Ok(ConformanceResult {
                        verdict: Verdict::NeedsHuman,
                        findings,
                    });
                }
            }
        }

        findings.push(format!("evidence covers task goal {}", task.goal_id));
        Ok(ConformanceResult {
            verdict: Verdict::Aligned,
            findings,
        })
    }

    fn resolve_agent<'a>(&'a self, supplied: &'a str) -> Result<&'a str> {
        let supplied = supplied.trim();
        if supplied.is_empty() {
            return self.agent.as_deref().ok_or_else(|| {
                LodestarError::Invalid(
                    "agent is required when LODESTAR_AGENT is not configured".to_string(),
                )
            });
        }
        if let Some(configured) = self.agent.as_deref() {
            if configured != supplied {
                return Err(LodestarError::Invalid(format!(
                    "agent {supplied} does not match configured identity {configured}"
                )));
            }
            Ok(configured)
        } else {
            Ok(supplied)
        }
    }

    fn validate_evidence_shape(&self, evidence: &ConformanceEvidence) -> Result<()> {
        if evidence.schema_version != 1 {
            return Err(LodestarError::Invalid(format!(
                "unsupported evidence schema version {}",
                evidence.schema_version
            )));
        }
        if evidence.started_at > evidence.ended_at {
            return Err(LodestarError::Invalid(
                "evidence start must not be after its end".to_string(),
            ));
        }
        if evidence.agent_id.trim().is_empty() {
            return Err(LodestarError::Invalid(
                "evidence agent must not be empty".to_string(),
            ));
        }
        if evidence.execution_ids.len() > MAX_EVIDENCE_EVENTS
            || evidence.commit_ids.len() > MAX_EVIDENCE_EVENTS
            || evidence.provenance.len() > MAX_EVIDENCE_PROVENANCE
            || evidence.summary.len() > MAX_EVIDENCE_SUMMARY_BYTES
        {
            return Err(LodestarError::Invalid(
                "evidence exceeds the bounded ADR-0009 contract".to_string(),
            ));
        }
        if !evidence
            .successful_execution_ids
            .iter()
            .all(|id| evidence.execution_ids.contains(id))
        {
            return Err(LodestarError::Invalid(
                "successful executions must be included in execution_ids".to_string(),
            ));
        }
        let agent_node_id = format!("agent:{}", evidence.agent_id);
        for event_id in evidence
            .execution_ids
            .iter()
            .chain(evidence.commit_ids.iter())
        {
            if !evidence.provenance.iter().any(|fact| {
                fact.source_id == agent_node_id
                    && fact.target_id == *event_id
                    && fact.relation == "observed"
            }) {
                return Err(LodestarError::Invalid(format!(
                    "event {event_id} lacks agent observation provenance"
                )));
            }
        }
        for changed_id in &evidence.changed_node_ids {
            if !evidence.provenance.iter().any(|fact| {
                fact.target_id == *changed_id
                    && matches!(fact.relation.as_str(), "modified" | "refactored")
                    && (evidence.execution_ids.contains(&fact.source_id)
                        || evidence.commit_ids.contains(&fact.source_id))
            }) {
                return Err(LodestarError::Invalid(format!(
                    "changed node {changed_id} lacks mutation provenance"
                )));
            }
        }
        for failed_id in &evidence.failed_node_ids {
            if !evidence.provenance.iter().any(|fact| {
                fact.target_id == *failed_id
                    && fact.relation == "failed_on"
                    && evidence.execution_ids.contains(&fact.source_id)
            }) {
                return Err(LodestarError::Invalid(format!(
                    "failed node {failed_id} lacks failure provenance"
                )));
            }
        }
        Ok(())
    }

    fn validate_claim_evidence(
        &self,
        task: &Task,
        agent: &str,
        evidence: &ConformanceEvidence,
        now: i64,
    ) -> Result<()> {
        self.validate_evidence_shape(evidence)?;
        if evidence.task_id.as_deref() != Some(task.id.as_str()) {
            return Err(LodestarError::Invalid(
                "evidence task_id does not identify the claimed task".to_string(),
            ));
        }
        if evidence.agent_id != agent || task.owner.as_deref() != Some(agent) {
            return Err(LodestarError::Invalid(
                "evidence agent does not own the task".to_string(),
            ));
        }
        if task.status != TaskStatus::Claimed || task.lease_expires_at.is_none_or(|end| end < now) {
            return Err(LodestarError::Invalid(
                "task does not have a live claim".to_string(),
            ));
        }
        let claim_started_at = task.claim_started_at.ok_or_else(|| {
            LodestarError::Invalid("task claim has no evidence-window start".to_string())
        })?;
        if evidence.started_at < claim_started_at
            || evidence.ended_at > now
            || evidence.ended_at > task.lease_expires_at.unwrap_or(evidence.ended_at)
        {
            return Err(LodestarError::Invalid(
                "evidence interval falls outside the live claim".to_string(),
            ));
        }
        Ok(())
    }

    // ---- knowledge / consolidation -----------------------------------------

    pub fn record_knowledge(
        &self,
        statement: &str,
        evidence: &str,
        half_life_hours: Option<f64>,
    ) -> Result<Knowledge> {
        let hl = half_life_hours.unwrap_or(decay::KNOWLEDGE_DEFAULT_HALF_LIFE_HOURS);
        self.store
            .record_knowledge(statement, evidence, hl, now_unix())
    }

    pub fn active_knowledge(&self) -> Result<Vec<Knowledge>> {
        self.store.active_knowledge(now_unix())
    }

    pub fn reconfirm_knowledge(&self, id: &str) -> Result<bool> {
        self.store.reconfirm_knowledge(id, now_unix())
    }

    pub fn prune_knowledge(&self) -> Result<usize> {
        self.store.prune_knowledge(now_unix())
    }

    /// Gated promotion of a discovered regularity into durable knowledge. Returns
    /// `None` (and stores nothing) unless the evidence clears the count and span
    /// thresholds — signal, not coincidence (ADR-0005).
    pub fn consolidate(
        &self,
        statement: &str,
        evidence_node_ids: &[String],
        first_seen: i64,
        last_seen: i64,
    ) -> Result<Option<Knowledge>> {
        if evidence_node_ids.len() < MIN_EVIDENCE_COUNT
            || (last_seen - first_seen) < MIN_EVIDENCE_SPAN_SECS
        {
            return Ok(None);
        }
        let evidence = serde_json::json!({
            "nodes": evidence_node_ids,
            "count": evidence_node_ids.len(),
            "first_seen": first_seen,
            "last_seen": last_seen,
        })
        .to_string();
        let k = self.store.record_knowledge(
            statement,
            &evidence,
            decay::KNOWLEDGE_DEFAULT_HALF_LIFE_HOURS,
            now_unix(),
        )?;
        Ok(Some(k))
    }

    pub fn stats(&self) -> Result<Stats> {
        self.store.stats(now_unix())
    }
}

/// Render the active constitution as committed-friendly markdown, grouped by
/// kind (invariants first — the hardest constraints lead).
fn render_constitution(goals: &[Goal]) -> String {
    let mut out = String::from("# Constitution\n\n");
    out.push_str("> Generated by Lodestar from the active Intent Plane. Do not edit by hand.\n\n");
    for (kind, heading) in [
        (GoalKind::Invariant, "## Invariants"),
        (GoalKind::Constraint, "## Constraints"),
        (GoalKind::Objective, "## Objectives"),
    ] {
        let group: Vec<&Goal> = goals.iter().filter(|g| g.kind == kind).collect();
        if group.is_empty() {
            continue;
        }
        out.push_str(heading);
        out.push_str("\n\n");
        for g in group {
            out.push_str(&format!("- **{}** (`{}`, v{})\n", g.title, g.id, g.version));
            out.push_str(&format!("  {}\n", g.statement));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    // Generated by AI (UnitTest MCP)
    use super::*;

    fn engine() -> Lodestar {
        // Deterministic: point the optional LLM at an unreachable endpoint so
        // model-optional paths (decompose, semantic conformance) take their
        // fallback regardless of any local server that happens to be running.
        Lodestar::open_in_memory().unwrap().with_llm(LlmClient {
            base_url: "http://127.0.0.1:1/v1".to_string(),
            model: "unreachable".to_string(),
            api_key: String::new(),
        })
    }

    #[test]
    fn constitution_returns_only_active_goals() {
        let e = engine();
        let g = e
            .define_goal(
                GoalKind::Invariant,
                "Zero-token write path",
                "No LLM on ingest",
                None,
            )
            .unwrap();
        e.supersede_goal(&g.id, "No LLM tokens on the write path", "clarity")
            .unwrap();
        let active = e.get_constitution().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].version, 2);
    }

    #[test]
    fn decompose_falls_back_to_single_task_without_llm() {
        let e = engine();
        let g = e
            .define_goal(
                GoalKind::Objective,
                "Add search",
                "Implement FTS search",
                None,
            )
            .unwrap();
        let tasks = e.decompose_goal(&g.id).unwrap();
        assert_eq!(tasks.len(), 1);
        assert!(tasks[0].title.contains("Add search"));
    }

    #[test]
    fn conformance_flags_ungoverned_as_aligned_and_governed_as_drift() {
        let e = engine();
        let g = e
            .define_goal(
                GoalKind::Constraint,
                "Auth stays typed",
                "no stringly auth",
                None,
            )
            .unwrap();
        e.link_goal_to_code(
            &g.id,
            &["artifact:src/auth.rs".into()],
            CodeBindingMode::Governed,
        )
        .unwrap();

        let free_evidence = test_evidence(None, "agent-a", "artifact:src/unrelated.rs");
        let free = e.check_conformance(&free_evidence, None).unwrap();
        assert_eq!(free.verdict, Verdict::Aligned);

        let drift_evidence = test_evidence(None, "agent-a", "artifact:src/auth.rs");
        let drift = e.check_conformance(&drift_evidence, None).unwrap();
        assert_eq!(drift.verdict, Verdict::Drift);
    }

    #[test]
    fn covering_task_without_evidence_needs_human_review() {
        let e = engine();
        let g = e
            .define_goal(GoalKind::Objective, "Auth typed", "harden auth", None)
            .unwrap();
        e.link_goal_to_code(
            &g.id,
            &["artifact:src/auth.rs".into()],
            CodeBindingMode::Governed,
        )
        .unwrap();
        let t = e.create_task(&g.id, "harden auth", "").unwrap();
        e.claim_task(&t.id, "agent-a", 300).unwrap();
        let claimed = e.store.get_task(&t.id).unwrap().unwrap();
        let evidence = ConformanceEvidence {
            schema_version: 1,
            task_id: Some(t.id.clone()),
            agent_id: "agent-a".into(),
            started_at: claimed.claim_started_at.unwrap(),
            ended_at: now_unix(),
            changed_node_ids: Vec::new(),
            failed_node_ids: Vec::new(),
            execution_ids: Vec::new(),
            successful_execution_ids: Vec::new(),
            commit_ids: Vec::new(),
            summary: "no activity".into(),
            provenance: Vec::new(),
        };
        let res = e.check_conformance(&evidence, Some(&t.id)).unwrap();
        assert_eq!(res.verdict, Verdict::NeedsHuman);
    }

    fn test_evidence(
        task_id: Option<String>,
        agent: &str,
        changed_node_id: &str,
    ) -> ConformanceEvidence {
        ConformanceEvidence {
            schema_version: 1,
            task_id,
            agent_id: agent.into(),
            started_at: 1,
            ended_at: 2,
            changed_node_ids: vec![changed_node_id.into()],
            failed_node_ids: Vec::new(),
            execution_ids: vec!["execution:proof".into()],
            successful_execution_ids: vec!["execution:proof".into()],
            commit_ids: Vec::new(),
            summary: format!("changed {changed_node_id}"),
            provenance: vec![
                EvidenceProvenance {
                    source_id: format!("agent:{agent}"),
                    target_id: "execution:proof".into(),
                    relation: "observed".into(),
                },
                EvidenceProvenance {
                    source_id: "execution:proof".into(),
                    target_id: changed_node_id.into(),
                    relation: "modified".into(),
                },
            ],
        }
    }

    #[test]
    fn consolidation_gate_rejects_coincidence_and_accepts_proven_signal() {
        let e = engine();
        // Too few, too fast: not promoted.
        let weak = e
            .consolidate("X flakes", &["execution:1".into()], 0, 10)
            .unwrap();
        assert!(weak.is_none());

        // Enough evidence across a wide span: promoted.
        let strong = e
            .consolidate(
                "changes to X break Y",
                &[
                    "execution:1".into(),
                    "execution:2".into(),
                    "execution:3".into(),
                ],
                0,
                10 * 24 * 3600,
            )
            .unwrap();
        assert!(strong.is_some());
        assert_eq!(e.active_knowledge().unwrap().len(), 1);
    }

    #[test]
    fn export_constitution_renders_grouped_markdown() {
        let e = engine();
        e.define_goal(
            GoalKind::Invariant,
            "Decay is the point",
            "edges fade",
            None,
        )
        .unwrap();
        let md = e.export_constitution(None).unwrap();
        assert!(md.contains("## Invariants"));
        assert!(md.contains("Decay is the point"));
    }
}

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
    ConformanceResult, Goal, GoalKind, GoalStatus, Knowledge, Task, TaskStatus, Verdict,
};
pub use store::{LodestarStore, Stats};

use llm::LlmClient;

/// Gating thresholds for consolidation (ADR-0005: don't launder coincidence).
const MIN_EVIDENCE_COUNT: usize = 3;
const MIN_EVIDENCE_SPAN_SECS: i64 = 3 * 24 * 3600; // proven across days, not one session

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
}

impl Lodestar {
    pub fn open(path: &str) -> Result<Self> {
        Ok(Lodestar {
            store: LodestarStore::new(db::open(path)?),
            llm: LlmClient::default(),
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        Ok(Lodestar {
            store: LodestarStore::new(db::open_in_memory()?),
            llm: LlmClient::default(),
        })
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

    pub fn link_goal_to_code(&self, goal_id: &str, node_ids: &[String]) -> Result<usize> {
        if !self.store.goal_exists(goal_id)? {
            return Err(LodestarError::NotFound(goal_id.to_string()));
        }
        self.store.link_goal_to_code(goal_id, node_ids)
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
        self.store.claim_task(id, agent, lease_secs, now_unix())
    }

    pub fn renew_lease(&self, id: &str, agent: &str, lease_secs: i64) -> Result<bool> {
        self.store.renew_lease(id, agent, lease_secs, now_unix())
    }

    pub fn release_task(&self, id: &str, agent: &str) -> Result<bool> {
        self.store.release_task(id, agent, now_unix())
    }

    pub fn block_task(&self, id: &str, blocked_by: Option<String>) -> Result<bool> {
        self.store
            .force_status(id, TaskStatus::Blocked, blocked_by, now_unix())
    }

    pub fn board(&self) -> Result<Vec<Task>> {
        self.store.board()
    }

    /// Complete a task (owner-guarded), then run conformance over the code its
    /// goal governs. A `violation` blocks the task instead of completing it.
    /// Returns `(completed, conformance)`.
    pub fn complete_task(&self, id: &str, agent: &str) -> Result<(bool, ConformanceResult)> {
        let now = now_unix();
        let task = self
            .store
            .get_task(id)?
            .ok_or_else(|| LodestarError::NotFound(id.to_string()))?;
        if !self
            .store
            .set_task_status(id, agent, TaskStatus::InReview, now)?
        {
            return Err(LodestarError::Invalid(format!(
                "task {id} is not claimed by {agent}"
            )));
        }
        let nodes = self.store.code_for_goal(&task.goal_id)?;
        let conformance = self.evaluate_conformance(&nodes, Some(id))?;
        if conformance.verdict == Verdict::Violation {
            self.store
                .force_status(id, TaskStatus::Blocked, None, now)?;
            Ok((false, conformance))
        } else {
            self.store.force_status(id, TaskStatus::Done, None, now)?;
            Ok((true, conformance))
        }
    }

    // ---- conformance -------------------------------------------------------

    /// Check whether a set of changed code nodes conforms to governing intent.
    pub fn check_conformance(
        &self,
        change_node_ids: &[String],
        task_id: Option<&str>,
    ) -> Result<ConformanceResult> {
        self.evaluate_conformance(change_node_ids, task_id)
    }

    fn evaluate_conformance(
        &self,
        change_node_ids: &[String],
        task_id: Option<&str>,
    ) -> Result<ConformanceResult> {
        let now = now_unix();
        let mut findings: Vec<String> = Vec::new();
        let mut governed: Vec<Goal> = Vec::new();
        for node in change_node_ids {
            for g in self.store.active_goals_for_node(node)? {
                if !governed.iter().any(|x| x.id == g.id) {
                    governed.push(g);
                }
            }
        }

        let verdict = if governed.is_empty() {
            findings.push("no governed code touched".to_string());
            Verdict::Aligned
        } else {
            let goal_ids: Vec<&str> = governed.iter().map(|g| g.id.as_str()).collect();
            findings.push(format!("governed by: {}", goal_ids.join(", ")));

            let sanctioned = match task_id {
                Some(tid) => match self.store.get_task(tid)? {
                    Some(t) => governed.iter().any(|g| g.id == t.goal_id),
                    None => false,
                },
                None => false,
            };

            if sanctioned {
                Verdict::Aligned
            } else {
                findings.push("governed code changed with no covering task".to_string());
                let mut verdict = Verdict::Drift;
                // Optional semantic escalation on normative goals (SLM; best-effort).
                for g in governed.iter().filter(|g| g.kind.is_normative()) {
                    if let Ok((v, rationale)) =
                        self.llm.judge(&g.statement, &change_node_ids.join(", "))
                    {
                        if v == "violation" {
                            verdict = Verdict::Violation;
                            findings.push(format!("SLM: violates {} — {rationale}", g.id));
                        }
                    }
                }
                verdict
            }
        };

        self.store
            .record_conformance(task_id, verdict, &findings.join("; "), now)?;
        Ok(ConformanceResult { verdict, findings })
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
        Lodestar::open_in_memory().unwrap()
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
        e.link_goal_to_code(&g.id, &["artifact:src/auth.rs".into()])
            .unwrap();

        let free = e
            .check_conformance(&["artifact:src/unrelated.rs".into()], None)
            .unwrap();
        assert_eq!(free.verdict, Verdict::Aligned);

        let drift = e
            .check_conformance(&["artifact:src/auth.rs".into()], None)
            .unwrap();
        assert_eq!(drift.verdict, Verdict::Drift);
    }

    #[test]
    fn conformance_is_aligned_when_task_covers_the_goal() {
        let e = engine();
        let g = e
            .define_goal(GoalKind::Constraint, "Auth typed", "no stringly auth", None)
            .unwrap();
        e.link_goal_to_code(&g.id, &["artifact:src/auth.rs".into()])
            .unwrap();
        let t = e.create_task(&g.id, "harden auth", "").unwrap();
        let res = e
            .check_conformance(&["artifact:src/auth.rs".into()], Some(&t.id))
            .unwrap();
        assert_eq!(res.verdict, Verdict::Aligned);
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

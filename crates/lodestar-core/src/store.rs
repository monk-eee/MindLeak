//! Durable storage and coordination for the Intent Plane.
//!
//! Goals (the constitution), tasks (the executive ledger, with the atomic
//! claim/lease compare-and-swap), the goal↔code seam, the conformance audit
//! trail, and consolidated learned knowledge. All coordination correctness comes
//! from guarded single-statement writes (see [`LodestarStore::claim_task`]).

use rusqlite::{params, Connection, Row};
use serde::Serialize;
use std::path::Path;

use crate::decay::ACTIVE_THRESHOLD;
use crate::error::{LodestarError, Result};
use crate::model::{
    CodeBinding, CodeBindingMode, Goal, GoalKind, GoalStatus, Knowledge, Task, TaskStatus, Verdict,
};
use crate::util::{short_hash, slugify};

const GOAL_COLS: &str =
    "id, slug, kind, title, statement, status, version, parent_id, superseded_by, reason, created_at";
const TASK_COLS: &str = "id, goal_id, parent_task_id, title, acceptance, status, owner, \
    claim_started_at, lease_expires_at, blocked_by, created_at, updated_at";
const KNOWLEDGE_COLS: &str =
    "id, statement, evidence, weight, half_life_hours, confirmed_at, created_at";

pub(crate) struct ConformanceAudit<'a> {
    pub evidence_schema_version: u32,
    pub evidence: &'a str,
    pub verdict: Verdict,
    pub findings: &'a str,
}

/// Summary counts for status displays.
#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub active_goals: i64,
    pub open_tasks: i64,
    pub claimed_tasks: i64,
    pub done_tasks: i64,
    pub active_knowledge: i64,
}

/// Counts removed by an explicitly confirmed intent-plane reset.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResetOutcome {
    pub goals_removed: usize,
    pub tasks_removed: usize,
    pub code_bindings_removed: usize,
    pub conformance_records_removed: usize,
    pub knowledge_removed: usize,
}

/// The persistent Intent Plane store.
pub struct LodestarStore {
    conn: Connection,
}

impl LodestarStore {
    pub fn new(conn: Connection) -> Self {
        LodestarStore { conn }
    }

    // ---- goals: the constitution -------------------------------------------

    pub fn goal_exists(&self, id: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(1) FROM goals WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn get_goal(&self, id: &str) -> Result<Option<Goal>> {
        let sql = format!("SELECT {GOAL_COLS} FROM goals WHERE id = ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_goal(row)?)),
            None => Ok(None),
        }
    }

    /// Define a new goal (constitution entry). Version 1, status active.
    pub fn define_goal(
        &self,
        kind: GoalKind,
        title: &str,
        statement: &str,
        parent: Option<String>,
        now: i64,
    ) -> Result<Goal> {
        let slug = slugify(title);
        let base = format!("goal:{slug}");
        let id = if self.goal_exists(&base)? {
            format!("goal:{slug}-{}", short_hash(statement))
        } else {
            base
        };
        let goal = Goal {
            id,
            slug,
            kind,
            title: title.to_string(),
            statement: statement.to_string(),
            status: GoalStatus::Active,
            version: 1,
            parent_id: parent,
            superseded_by: None,
            reason: None,
            created_at: now,
        };
        self.insert_goal(&goal)?;
        Ok(goal)
    }

    fn insert_goal(&self, g: &Goal) -> Result<()> {
        self.conn.execute(
            "INSERT INTO goals
                (id, slug, kind, title, statement, status, version, parent_id, superseded_by, reason, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                g.id,
                g.slug,
                g.kind.as_str(),
                g.title,
                g.statement,
                g.status.as_str(),
                g.version,
                g.parent_id,
                g.superseded_by,
                g.reason,
                g.created_at,
            ],
        )?;
        Ok(())
    }

    /// Supersede a goal: write a new active version and mark the old one
    /// superseded. Intent changes only through this explicit, attributed step.
    pub fn supersede_goal(
        &self,
        old_id: &str,
        new_statement: &str,
        reason: &str,
        now: i64,
    ) -> Result<Goal> {
        let old = self
            .get_goal(old_id)?
            .ok_or_else(|| LodestarError::NotFound(old_id.to_string()))?;
        let version = old.version + 1;
        let new_id = format!("goal:{}-v{}", old.slug, version);
        let new_goal = Goal {
            id: new_id.clone(),
            slug: old.slug.clone(),
            kind: old.kind,
            title: old.title.clone(),
            statement: new_statement.to_string(),
            status: GoalStatus::Active,
            version,
            parent_id: old.parent_id.clone(),
            superseded_by: None,
            reason: Some(reason.to_string()),
            created_at: now,
        };
        self.insert_goal(&new_goal)?;
        self.conn.execute(
            "UPDATE goals SET status = 'superseded', superseded_by = ?2 WHERE id = ?1",
            params![old_id, new_id],
        )?;
        Ok(new_goal)
    }

    pub fn goals_by_status(&self, status: GoalStatus) -> Result<Vec<Goal>> {
        let sql =
            format!("SELECT {GOAL_COLS} FROM goals WHERE status = ?1 ORDER BY kind, created_at");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![status.as_str()], row_to_goal)?;
        collect(rows)
    }

    // ---- goal ↔ code seam --------------------------------------------------

    pub fn link_goal_to_code(
        &self,
        goal_id: &str,
        node_ids: &[String],
        mode: CodeBindingMode,
    ) -> Result<usize> {
        let mut linked = 0;
        for node in node_ids {
            linked += self.conn.execute(
                "INSERT INTO goal_code (goal_id, node_id, mode) VALUES (?1, ?2, ?3)
                 ON CONFLICT(goal_id, node_id) DO UPDATE SET mode = excluded.mode",
                params![goal_id, node, mode.as_str()],
            )?;
        }
        Ok(linked)
    }

    pub fn code_for_goal(&self, goal_id: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT node_id FROM goal_code WHERE goal_id = ?1")?;
        let rows = stmt.query_map(params![goal_id], |r| r.get::<_, String>(0))?;
        collect(rows)
    }

    /// Active goal policies governing a given code node.
    pub fn active_bindings_for_node(&self, node_id: &str) -> Result<Vec<CodeBinding>> {
        let sql = format!(
            "SELECT {}, c.mode FROM goal_code c JOIN goals g ON g.id = c.goal_id
             WHERE c.node_id = ?1 AND g.status = 'active'",
            GOAL_COLS
                .split(", ")
                .map(|c| format!("g.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![node_id], |row| {
            let mode: String = row.get(11)?;
            Ok(CodeBinding {
                goal: row_to_goal(row)?,
                mode: CodeBindingMode::from_tag(&mode).unwrap_or(CodeBindingMode::Governed),
            })
        })?;
        collect(rows)
    }

    // ---- tasks: the executive ledger ---------------------------------------

    pub fn create_task(
        &self,
        goal_id: &str,
        title: &str,
        acceptance: &str,
        parent: Option<String>,
        now: i64,
    ) -> Result<Task> {
        if !self.goal_exists(goal_id)? {
            return Err(LodestarError::NotFound(goal_id.to_string()));
        }
        let id = format!("task:{}", short_hash(&format!("{goal_id}|{title}|{now}")));
        let task = Task {
            id,
            goal_id: goal_id.to_string(),
            parent_task_id: parent,
            title: title.to_string(),
            acceptance: acceptance.to_string(),
            status: TaskStatus::Open,
            owner: None,
            claim_started_at: None,
            lease_expires_at: None,
            blocked_by: None,
            created_at: now,
            updated_at: now,
        };
        self.conn.execute(
            "INSERT INTO tasks
                     (id, goal_id, parent_task_id, title, acceptance, status, owner, claim_started_at, lease_expires_at, blocked_by, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'open', NULL, NULL, NULL, NULL, ?6, ?6)",
            params![
                task.id,
                task.goal_id,
                task.parent_task_id,
                task.title,
                task.acceptance,
                now,
            ],
        )?;
        Ok(task)
    }

    pub fn get_task(&self, id: &str) -> Result<Option<Task>> {
        let sql = format!("SELECT {TASK_COLS} FROM tasks WHERE id = ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_task(row)?)),
            None => Ok(None),
        }
    }

    /// Claim a task: the coordination primitive. A guarded compare-and-swap —
    /// succeeds only if the task is open, its lease has expired, or the caller
    /// already owns it. Returns true iff this caller won the claim.
    pub fn claim_task(&self, id: &str, agent: &str, lease_secs: i64, now: i64) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE tasks
                SET status = 'claimed', owner = ?2,
                    claim_started_at = CASE
                        WHEN status = 'claimed' AND owner = ?2 AND lease_expires_at >= ?4
                        THEN claim_started_at ELSE ?4 END,
                    lease_expires_at = ?3, updated_at = ?4
              WHERE id = ?1
                AND (status = 'open'
                     OR (status = 'claimed' AND lease_expires_at < ?4)
                     OR (status = 'claimed' AND owner = ?2))",
            params![id, agent, now + lease_secs, now],
        )?;
        Ok(changed == 1)
    }

    /// Extend the lease on a task the caller still owns (heartbeat).
    pub fn renew_lease(&self, id: &str, agent: &str, lease_secs: i64, now: i64) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE tasks SET lease_expires_at = ?3, updated_at = ?4
              WHERE id = ?1 AND owner = ?2 AND status = 'claimed'",
            params![id, agent, now + lease_secs, now],
        )?;
        Ok(changed == 1)
    }

    /// Unconditional status transition (used by the facade after conformance,
    /// and to block/abandon). Not owner-guarded on purpose.
    pub fn force_status(
        &self,
        id: &str,
        status: TaskStatus,
        blocked_by: Option<String>,
        now: i64,
    ) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE tasks SET status = ?2, blocked_by = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, status.as_str(), blocked_by, now],
        )?;
        Ok(changed == 1)
    }

    /// Release a claim back to `open` (owner-guarded).
    pub fn release_task(&self, id: &str, agent: &str, now: i64) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE tasks SET status = 'open', owner = NULL, claim_started_at = NULL,
                                                            lease_expires_at = NULL, updated_at = ?3
              WHERE id = ?1 AND owner = ?2",
            params![id, agent, now],
        )?;
        Ok(changed == 1)
    }

    /// The next unblocked, claimable task (open or lease-expired), oldest first.
    pub fn next_task(&self, now: i64) -> Result<Option<Task>> {
        let sql = format!(
            "SELECT {TASK_COLS} FROM tasks
              WHERE blocked_by IS NULL
                AND (status = 'open' OR (status = 'claimed' AND lease_expires_at < ?1))
              ORDER BY created_at ASC LIMIT 1"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params![now])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_task(row)?)),
            None => Ok(None),
        }
    }

    pub fn board(&self) -> Result<Vec<Task>> {
        let sql = format!("SELECT {TASK_COLS} FROM tasks ORDER BY created_at ASC");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_task)?;
        collect(rows)
    }

    // ---- conformance audit -------------------------------------------------

    pub(crate) fn record_conformance(
        &self,
        task_id: Option<&str>,
        audit: ConformanceAudit<'_>,
        now: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO conformance
                 (task_id, evidence_schema_version, evidence, verdict, findings, checked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                task_id,
                audit.evidence_schema_version,
                audit.evidence,
                audit.verdict.as_str(),
                audit.findings,
                now
            ],
        )?;
        Ok(())
    }

    /// Atomically leave `claimed` and persist the evidence-backed verdict.
    pub(crate) fn record_conformance_and_transition(
        &self,
        task_id: &str,
        agent: &str,
        audit: ConformanceAudit<'_>,
        target_status: TaskStatus,
        now: i64,
    ) -> Result<bool> {
        let transaction = self.conn.unchecked_transaction()?;
        let changed = transaction.execute(
            "UPDATE tasks
                SET status = ?3, lease_expires_at = NULL, updated_at = ?4
              WHERE id = ?1 AND owner = ?2 AND status = 'claimed'
                AND lease_expires_at >= ?4",
            params![task_id, agent, target_status.as_str(), now],
        )?;
        if changed != 1 {
            return Ok(false);
        }
        transaction.execute(
            "INSERT INTO conformance
                 (task_id, evidence_schema_version, evidence, verdict, findings, checked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                task_id,
                audit.evidence_schema_version,
                audit.evidence,
                audit.verdict.as_str(),
                audit.findings,
                now
            ],
        )?;
        transaction.commit()?;
        Ok(true)
    }

    // ---- knowledge: consolidated experience --------------------------------

    /// Insert or reconfirm a knowledge node. Re-recording the same statement
    /// bumps its weight and resets its revalidation clock.
    pub fn record_knowledge(
        &self,
        statement: &str,
        evidence: &str,
        half_life_hours: f64,
        now: i64,
    ) -> Result<Knowledge> {
        let id = format!("knowledge:{}", short_hash(statement));
        self.conn.execute(
            "INSERT INTO knowledge (id, statement, evidence, weight, half_life_hours, confirmed_at, created_at)
             VALUES (?1, ?2, ?3, 1.0, ?4, ?5, ?5)
             ON CONFLICT(id) DO UPDATE SET
                 weight = MIN(1.0, knowledge.weight + 0.1),
                 evidence = excluded.evidence,
                 confirmed_at = excluded.confirmed_at",
            params![id, statement, evidence, half_life_hours, now],
        )?;
        self.get_knowledge(&id)?
            .ok_or_else(|| LodestarError::Invalid("knowledge vanished after insert".into()))
    }

    pub fn get_knowledge(&self, id: &str) -> Result<Option<Knowledge>> {
        let sql = format!("SELECT {KNOWLEDGE_COLS} FROM knowledge WHERE id = ?1");
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_knowledge(row)?)),
            None => Ok(None),
        }
    }

    /// Re-confirm a knowledge node with fresh evidence (resets the decay clock).
    pub fn reconfirm_knowledge(&self, id: &str, now: i64) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE knowledge SET confirmed_at = ?2, weight = MIN(1.0, weight + 0.1) WHERE id = ?1",
            params![id, now],
        )?;
        Ok(changed == 1)
    }

    /// Knowledge still above the active threshold, strongest first.
    pub fn active_knowledge(&self, now: i64) -> Result<Vec<Knowledge>> {
        let sql = format!(
            "SELECT {KNOWLEDGE_COLS} FROM knowledge
              WHERE effective_weight(weight, half_life_hours, confirmed_at, ?1) >= ?2
              ORDER BY effective_weight(weight, half_life_hours, confirmed_at, ?1) DESC"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![now, ACTIVE_THRESHOLD], row_to_knowledge)?;
        collect(rows)
    }

    /// Purge knowledge that decayed below the threshold without reconfirmation.
    pub fn prune_knowledge(&self, now: i64) -> Result<usize> {
        let removed = self.conn.execute(
            "DELETE FROM knowledge WHERE effective_weight(weight, half_life_hours, confirmed_at, ?1) < ?2",
            params![now, ACTIVE_THRESHOLD],
        )?;
        Ok(removed)
    }

    // ---- stats -------------------------------------------------------------

    pub fn stats(&self, now: i64) -> Result<Stats> {
        let active_goals: i64 = self.conn.query_row(
            "SELECT COUNT(1) FROM goals WHERE status = 'active'",
            [],
            |r| r.get(0),
        )?;
        let open_tasks: i64 = self.conn.query_row(
            "SELECT COUNT(1) FROM tasks WHERE status = 'open'",
            [],
            |r| r.get(0),
        )?;
        let claimed_tasks: i64 = self.conn.query_row(
            "SELECT COUNT(1) FROM tasks WHERE status = 'claimed'",
            [],
            |r| r.get(0),
        )?;
        let done_tasks: i64 = self.conn.query_row(
            "SELECT COUNT(1) FROM tasks WHERE status = 'done'",
            [],
            |r| r.get(0),
        )?;
        let active_knowledge: i64 = self.conn.query_row(
            "SELECT COUNT(1) FROM knowledge WHERE effective_weight(weight, half_life_hours, confirmed_at, ?1) >= ?2",
            params![now, ACTIVE_THRESHOLD],
            |r| r.get(0),
        )?;
        Ok(Stats {
            active_goals,
            open_tasks,
            claimed_tasks,
            done_tasks,
            active_knowledge,
        })
    }

    /// Create a consistent SQLite backup while this store remains online.
    pub fn backup_database(&self, destination: &Path) -> Result<()> {
        mindleak_storage::backup_database(&self.conn, destination)?;
        Ok(())
    }

    /// Clear all durable intent only after the exact, plane-specific token.
    pub fn reset_database(&self, confirmation: &str) -> Result<ResetOutcome> {
        if confirmation != "RESET LODESTAR" {
            return Err(LodestarError::Invalid(
                "intent reset requires exact confirmation token: RESET LODESTAR".to_string(),
            ));
        }

        let transaction = self.conn.unchecked_transaction()?;
        let conformance_records_removed = transaction.execute("DELETE FROM conformance", [])?;
        let code_bindings_removed = transaction.execute("DELETE FROM goal_code", [])?;
        let tasks_removed = transaction.execute("DELETE FROM tasks", [])?;
        let knowledge_removed = transaction.execute("DELETE FROM knowledge", [])?;
        let goals_removed = transaction.execute("DELETE FROM goals", [])?;
        transaction.commit()?;

        Ok(ResetOutcome {
            goals_removed,
            tasks_removed,
            code_bindings_removed,
            conformance_records_removed,
            knowledge_removed,
        })
    }
}

fn collect<T>(rows: impl Iterator<Item = rusqlite::Result<T>>) -> Result<Vec<T>> {
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

fn row_to_goal(row: &Row) -> rusqlite::Result<Goal> {
    let kind: String = row.get(2)?;
    let status: String = row.get(5)?;
    Ok(Goal {
        id: row.get(0)?,
        slug: row.get(1)?,
        kind: GoalKind::from_tag(&kind).unwrap_or(GoalKind::Objective),
        title: row.get(3)?,
        statement: row.get(4)?,
        status: GoalStatus::from_tag(&status).unwrap_or(GoalStatus::Active),
        version: row.get(6)?,
        parent_id: row.get(7)?,
        superseded_by: row.get(8)?,
        reason: row.get(9)?,
        created_at: row.get(10)?,
    })
}

fn row_to_task(row: &Row) -> rusqlite::Result<Task> {
    let status: String = row.get(5)?;
    Ok(Task {
        id: row.get(0)?,
        goal_id: row.get(1)?,
        parent_task_id: row.get(2)?,
        title: row.get(3)?,
        acceptance: row.get(4)?,
        status: TaskStatus::from_tag(&status).unwrap_or(TaskStatus::Open),
        owner: row.get(6)?,
        claim_started_at: row.get(7)?,
        lease_expires_at: row.get(8)?,
        blocked_by: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn row_to_knowledge(row: &Row) -> rusqlite::Result<Knowledge> {
    Ok(Knowledge {
        id: row.get(0)?,
        statement: row.get(1)?,
        evidence: row.get(2)?,
        weight: row.get(3)?,
        half_life_hours: row.get(4)?,
        confirmed_at: row.get(5)?,
        created_at: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    // Generated by AI (UnitTest MCP)
    use super::*;
    use crate::db;

    const NOW: i64 = 1_000_000;
    const HOUR: i64 = 3600;

    fn store() -> LodestarStore {
        LodestarStore::new(db::open_in_memory().unwrap())
    }

    fn goal(s: &LodestarStore) -> Goal {
        s.define_goal(
            GoalKind::Objective,
            "Ship the thing",
            "Do it well",
            None,
            NOW,
        )
        .unwrap()
    }

    #[test]
    fn claim_is_a_compare_and_swap() {
        let s = store();
        let g = goal(&s);
        let t = s.create_task(&g.id, "task", "", None, NOW).unwrap();

        assert!(s.claim_task(&t.id, "alice", 60, NOW).unwrap()); // open -> alice wins
        assert!(!s.claim_task(&t.id, "bob", 60, NOW).unwrap()); // live claim -> bob loses
        assert!(s.claim_task(&t.id, "alice", 60, NOW).unwrap()); // owner re-claim idempotent

        let held = s.get_task(&t.id).unwrap().unwrap();
        assert_eq!(held.owner.as_deref(), Some("alice"));
        assert_eq!(held.status, TaskStatus::Claimed);
    }

    #[test]
    fn expired_lease_is_reclaimable_by_another_agent() {
        let s = store();
        let g = goal(&s);
        let t = s.create_task(&g.id, "task", "", None, NOW).unwrap();

        assert!(s.claim_task(&t.id, "alice", 60, NOW).unwrap());
        // 2 hours later, alice's 60s lease has long expired.
        let later = NOW + 2 * HOUR;
        assert!(s.claim_task(&t.id, "bob", 60, later).unwrap());
        assert_eq!(
            s.get_task(&t.id).unwrap().unwrap().owner.as_deref(),
            Some("bob")
        );
    }

    #[test]
    fn only_owner_can_record_conformance_or_release() {
        let s = store();
        let g = goal(&s);
        let t = s.create_task(&g.id, "task", "", None, NOW).unwrap();
        s.claim_task(&t.id, "alice", 60, NOW).unwrap();

        assert!(!s
            .record_conformance_and_transition(
                &t.id,
                "bob",
                ConformanceAudit {
                    evidence_schema_version: 1,
                    evidence: "{}",
                    verdict: Verdict::NeedsHuman,
                    findings: "missing evidence",
                },
                TaskStatus::InReview,
                NOW,
            )
            .unwrap());
        assert!(!s.release_task(&t.id, "bob", NOW).unwrap());
        assert!(s
            .record_conformance_and_transition(
                &t.id,
                "alice",
                ConformanceAudit {
                    evidence_schema_version: 1,
                    evidence: "{}",
                    verdict: Verdict::NeedsHuman,
                    findings: "missing evidence",
                },
                TaskStatus::InReview,
                NOW,
            )
            .unwrap());
    }

    #[test]
    fn supersede_bumps_version_and_retires_old() {
        let s = store();
        let g = goal(&s);
        let v2 = s
            .supersede_goal(&g.id, "Do it even better", "learned more", NOW)
            .unwrap();

        assert_eq!(v2.version, 2);
        let active = s.goals_by_status(GoalStatus::Active).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, v2.id);
        let old = s.get_goal(&g.id).unwrap().unwrap();
        assert_eq!(old.status, GoalStatus::Superseded);
        assert_eq!(old.superseded_by.as_deref(), Some(v2.id.as_str()));
    }

    #[test]
    fn backup_database_preserves_constitution() {
        let store = store();
        let goal = goal(&store);
        let path =
            std::env::temp_dir().join(format!("lodestar-store-backup-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);

        store.backup_database(&path).unwrap();

        let restored = LodestarStore::new(db::open(path.to_str().unwrap()).unwrap());
        assert_eq!(
            restored.get_goal(&goal.id).unwrap().unwrap().title,
            goal.title
        );
        drop(restored);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn reset_database_requires_exact_token_and_clears_durable_intent() {
        let store = store();
        let goal = goal(&store);
        let task = store
            .create_task(&goal.id, "Implement reset", "all state gone", None, NOW)
            .unwrap();
        store
            .link_goal_to_code(
                &goal.id,
                &["artifact:src/lib.rs".to_string()],
                CodeBindingMode::Governed,
            )
            .unwrap();
        store.claim_task(&task.id, "agent-a", 60, NOW).unwrap();
        store
            .record_conformance_and_transition(
                &task.id,
                "agent-a",
                ConformanceAudit {
                    evidence_schema_version: 1,
                    evidence: "{}",
                    verdict: Verdict::Aligned,
                    findings: "",
                },
                TaskStatus::Done,
                NOW,
            )
            .unwrap();
        store
            .record_knowledge("keep tests focused", "{}", 720.0, NOW)
            .unwrap();

        assert!(store.reset_database("RESET MINDLEAK").is_err());
        assert!(store.get_goal(&goal.id).unwrap().is_some());

        let outcome = store.reset_database("RESET LODESTAR").unwrap();
        assert_eq!(outcome.goals_removed, 1);
        assert_eq!(outcome.tasks_removed, 1);
        assert_eq!(outcome.code_bindings_removed, 1);
        assert_eq!(outcome.conformance_records_removed, 1);
        assert_eq!(outcome.knowledge_removed, 1);
        assert_eq!(store.stats(NOW).unwrap().active_goals, 0);
        assert!(store
            .define_goal(GoalKind::Objective, "New goal", "usable", None, NOW)
            .is_ok());
    }

    #[test]
    fn goal_code_seam_resolves_active_governors() {
        let s = store();
        let g = goal(&s);
        s.link_goal_to_code(
            &g.id,
            &["artifact:src/x.rs".into()],
            CodeBindingMode::Governed,
        )
        .unwrap();
        let bindings = s.active_bindings_for_node("artifact:src/x.rs").unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].goal.id, g.id);
        assert_eq!(bindings[0].mode, CodeBindingMode::Governed);
    }

    #[test]
    fn next_task_skips_blocked_and_prefers_oldest() {
        let s = store();
        let g = goal(&s);
        let first = s.create_task(&g.id, "first", "", None, NOW).unwrap();
        let second = s.create_task(&g.id, "second", "", None, NOW + 1).unwrap();
        s.force_status(&second.id, TaskStatus::Blocked, Some("x".into()), NOW)
            .unwrap();
        let next = s.next_task(NOW + 10).unwrap().unwrap();
        assert_eq!(next.id, first.id);
    }

    #[test]
    fn knowledge_revalidation_extends_life() {
        let s = store();
        let k = s.record_knowledge("X breaks Y", "{}", 720.0, NOW).unwrap();
        // Far in the future, unconfirmed knowledge has faded out.
        let far = NOW + 200 * 24 * HOUR;
        assert!(s.active_knowledge(far).unwrap().is_empty());
        // Reconfirming resets the clock so it is active again at that time.
        assert!(s.reconfirm_knowledge(&k.id, far).unwrap());
        assert_eq!(s.active_knowledge(far).unwrap().len(), 1);
    }
}

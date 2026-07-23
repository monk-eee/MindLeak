//! Durable storage and coordination for the Intent Plane.
//!
//! Goals (the constitution), tasks (the executive ledger, with the atomic
//! claim/lease compare-and-swap), the goal↔code seam, the conformance audit
//! trail, and consolidated learned knowledge. All coordination correctness comes
//! from guarded single-statement writes (see [`LodestarStore::claim_task`]).

use rusqlite::{params, Connection, OptionalExtension, Row, Transaction, TransactionBehavior};
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
        self.create_task_after(goal_id, title, acceptance, parent, None, now)
    }

    pub fn create_task_after(
        &self,
        goal_id: &str,
        title: &str,
        acceptance: &str,
        parent: Option<String>,
        blocked_by: Option<String>,
        now: i64,
    ) -> Result<Task> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if !goal_exists_on(&transaction, goal_id)? {
            return Err(LodestarError::NotFound(goal_id.to_string()));
        }
        let id = format!("task:{}", short_hash(&format!("{goal_id}|{title}|{now}")));
        let predecessor_id = blocked_by;
        let blocked_by = match predecessor_id.as_deref() {
            Some(blocker_id) => (!validate_dependency_on(&transaction, &id, goal_id, blocker_id)?)
                .then(|| blocker_id.to_string()),
            None => None,
        };
        let status = if blocked_by.is_some() {
            TaskStatus::Blocked
        } else {
            TaskStatus::Open
        };
        let task = Task {
            id,
            goal_id: goal_id.to_string(),
            parent_task_id: parent,
            title: title.to_string(),
            acceptance: acceptance.to_string(),
            status,
            owner: None,
            claim_started_at: None,
            lease_expires_at: None,
            blocked_by,
            created_at: now,
            updated_at: now,
        };
        transaction.execute(
            "INSERT INTO tasks
                     (id, goal_id, parent_task_id, title, acceptance, status, owner, claim_started_at, lease_expires_at, blocked_by, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, NULL, ?7, ?8, ?8)",
            params![
                task.id,
                task.goal_id,
                task.parent_task_id,
                task.title,
                task.acceptance,
                task.status.as_str(),
                task.blocked_by,
                now,
            ],
        )?;
        if let Some(predecessor_id) = predecessor_id {
            transaction.execute(
                "INSERT INTO task_handoffs (predecessor_id, successor_id, created_at)
                 VALUES (?1, ?2, ?3)",
                params![predecessor_id, task.id, now],
            )?;
        }
        transaction.commit()?;
        Ok(task)
    }

    pub fn get_task(&self, id: &str) -> Result<Option<Task>> {
        get_task_on(&self.conn, id)
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
                                AND blocked_by IS NULL
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
                            WHERE id = ?1 AND owner = ?2 AND status = 'claimed'
                                AND blocked_by IS NULL",
            params![id, agent, now + lease_secs, now],
        )?;
        Ok(changed == 1)
    }

    /// Mark a task blocked, optionally on one validated predecessor. Blocking
    /// clears any live claim so release cannot reopen it around the dependency.
    pub fn block_task(&self, id: &str, blocked_by: Option<String>, now: i64) -> Result<bool> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let task = get_task_on(&transaction, id)?
            .ok_or_else(|| LodestarError::NotFound(id.to_string()))?;
        if matches!(task.status, TaskStatus::Done | TaskStatus::Abandoned) {
            return Err(LodestarError::Invalid(format!(
                "task {id} is terminal and cannot be blocked"
            )));
        }
        let existing_predecessor: Option<String> = transaction
            .query_row(
                "SELECT predecessor_id FROM task_handoffs WHERE successor_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        let dependency = match blocked_by {
            Some(blocker_id) => {
                if existing_predecessor
                    .as_deref()
                    .is_some_and(|id| id != blocker_id)
                {
                    return Err(LodestarError::Invalid(format!(
                        "task {id} already belongs to a different handoff chain"
                    )));
                }
                if validate_dependency_on(&transaction, id, &task.goal_id, &blocker_id)? {
                    return Err(LodestarError::Invalid(format!(
                        "predecessor {blocker_id} is already complete"
                    )));
                }
                Some(blocker_id)
            }
            None => None,
        };
        let changed = transaction.execute(
            "UPDATE tasks
             SET status = 'blocked', owner = NULL, claim_started_at = NULL,
                 lease_expires_at = NULL, blocked_by = ?2, updated_at = ?3
             WHERE id = ?1",
            params![id, dependency, now],
        )?;
        if let Some(predecessor_id) = dependency.as_ref() {
            transaction.execute(
                "INSERT OR IGNORE INTO task_handoffs
                     (predecessor_id, successor_id, created_at)
                 VALUES (?1, ?2, ?3)",
                params![predecessor_id, id, now],
            )?;
        }
        transaction.commit()?;
        Ok(changed == 1)
    }

    /// Return a stranded task to `open` so it can be reclaimed. A task is
    /// stranded when it is `in_review` (a drift or needs-human completion
    /// outcome) or manually `blocked` with no live predecessor gate. Refuses to
    /// bypass a handoff dependency (`blocked` with a `blocked_by`), to disturb an
    /// active claim (`claimed` — use `release_task`), or to revive terminal work.
    pub fn reopen_task(&self, id: &str, now: i64) -> Result<bool> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let task = get_task_on(&transaction, id)?
            .ok_or_else(|| LodestarError::NotFound(id.to_string()))?;
        match task.status {
            TaskStatus::InReview => {}
            TaskStatus::Blocked if task.blocked_by.is_none() => {}
            TaskStatus::Blocked => {
                return Err(LodestarError::Invalid(format!(
                    "task {id} is gated by predecessor {}; it opens when that predecessor completes",
                    task.blocked_by.as_deref().unwrap_or_default()
                )));
            }
            other => {
                return Err(LodestarError::Invalid(format!(
                    "task {id} is {} and cannot be reopened",
                    other.as_str()
                )));
            }
        }
        let changed = transaction.execute(
            "UPDATE tasks
             SET status = 'open', owner = NULL, claim_started_at = NULL,
                 lease_expires_at = NULL, blocked_by = NULL, updated_at = ?2
             WHERE id = ?1",
            params![id, now],
        )?;
        transaction.commit()?;
        Ok(changed == 1)
    }

    /// Release a claim back to `open` (owner-guarded).
    pub fn release_task(&self, id: &str, agent: &str, now: i64) -> Result<bool> {
        let changed = self.conn.execute(
            "UPDATE tasks SET status = 'open', owner = NULL, claim_started_at = NULL,
                                                            lease_expires_at = NULL, updated_at = ?3
                            WHERE id = ?1 AND owner = ?2 AND status = 'claimed'
                                AND blocked_by IS NULL",
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
        let expected_status = match audit.verdict {
            Verdict::Aligned => TaskStatus::Done,
            Verdict::Violation => TaskStatus::Blocked,
            Verdict::Drift | Verdict::NeedsHuman => TaskStatus::InReview,
        };
        if target_status != expected_status {
            return Err(LodestarError::Invalid(format!(
                "verdict {} requires status {}, not {}",
                audit.verdict.as_str(),
                expected_status.as_str(),
                target_status.as_str()
            )));
        }
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        if target_status == TaskStatus::Done {
            let successors: i64 = transaction.query_row(
                "SELECT COUNT(1) FROM task_handoffs WHERE predecessor_id = ?1",
                params![task_id],
                |row| row.get(0),
            )?;
            if successors > 1 {
                return Err(LodestarError::Invalid(format!(
                    "task {task_id} has {successors} successors; progressive handoff must be linear"
                )));
            }
        }
        let changed = transaction.execute(
            "UPDATE tasks
                SET status = ?3, owner = NULL, claim_started_at = NULL,
                    lease_expires_at = NULL, updated_at = ?4
              WHERE id = ?1 AND owner = ?2 AND status = 'claimed'
                AND lease_expires_at >= ?4 AND blocked_by IS NULL",
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
        if target_status == TaskStatus::Done {
            transaction.execute(
                "UPDATE tasks
                 SET status = 'open', owner = NULL, claim_started_at = NULL,
                     lease_expires_at = NULL, blocked_by = NULL, updated_at = ?2
                 WHERE id = (
                     SELECT successor_id FROM task_handoffs WHERE predecessor_id = ?1
                 )
                   AND blocked_by = ?1 AND status = 'blocked'",
                params![task_id, now],
            )?;
        }
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
                 half_life_hours = excluded.half_life_hours,
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

fn goal_exists_on(connection: &Connection, id: &str) -> Result<bool> {
    let count: i64 = connection.query_row(
        "SELECT COUNT(1) FROM goals WHERE id = ?1",
        params![id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn get_task_on(connection: &Connection, id: &str) -> Result<Option<Task>> {
    let sql = format!("SELECT {TASK_COLS} FROM tasks WHERE id = ?1");
    Ok(connection
        .query_row(&sql, params![id], row_to_task)
        .optional()?)
}

/// Validate one linear same-goal predecessor. Returns true only when the
/// predecessor already reached evidence-backed aligned completion.
fn validate_dependency_on(
    connection: &Connection,
    task_id: &str,
    goal_id: &str,
    blocker_id: &str,
) -> Result<bool> {
    if task_id == blocker_id {
        return Err(LodestarError::Invalid(
            "a task cannot depend on itself".to_string(),
        ));
    }
    let blocker = get_task_on(connection, blocker_id)?
        .ok_or_else(|| LodestarError::NotFound(blocker_id.to_string()))?;
    if blocker.goal_id != goal_id {
        return Err(LodestarError::Invalid(format!(
            "task {blocker_id} serves a different goal"
        )));
    }
    let existing_successor: Option<String> = connection
        .query_row(
            "SELECT successor_id FROM task_handoffs
             WHERE predecessor_id = ?1 AND successor_id <> ?2",
            params![blocker_id, task_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(existing) = existing_successor {
        return Err(LodestarError::Invalid(format!(
            "task {blocker_id} already hands off to {existing}; handoffs must be linear"
        )));
    }
    let existing_predecessor: Option<String> = connection
        .query_row(
            "SELECT predecessor_id FROM task_handoffs
             WHERE successor_id = ?1 AND predecessor_id <> ?2",
            params![task_id, blocker_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(existing) = existing_predecessor {
        return Err(LodestarError::Invalid(format!(
            "task {task_id} already follows {existing}; handoffs must be linear"
        )));
    }
    let cyclic: bool = connection.query_row(
        "WITH RECURSIVE chain(id) AS (
             SELECT successor_id FROM task_handoffs WHERE predecessor_id = ?1
             UNION
             SELECT handoff.successor_id
             FROM task_handoffs handoff JOIN chain
               ON handoff.predecessor_id = chain.id
         )
         SELECT EXISTS(SELECT 1 FROM chain WHERE id = ?2)",
        params![task_id, blocker_id],
        |row| row.get(0),
    )?;
    if cyclic {
        return Err(LodestarError::Invalid(
            "task dependency would create a cycle".to_string(),
        ));
    }
    if blocker.status == TaskStatus::Done {
        let aligned: bool = connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM conformance
                 WHERE task_id = ?1 AND verdict = 'aligned'
             )",
            params![blocker_id],
            |row| row.get(0),
        )?;
        return if aligned {
            Ok(true)
        } else {
            Err(LodestarError::Invalid(format!(
                "task {blocker_id} is done without aligned completion evidence"
            )))
        };
    }
    if blocker.status == TaskStatus::Abandoned {
        return Err(LodestarError::Invalid(format!(
            "task {blocker_id} is abandoned and cannot be a predecessor"
        )));
    }
    Ok(false)
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
    use std::path::PathBuf;
    use std::sync::{Arc, Barrier};

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

    fn complete_aligned(store: &LodestarStore, task_id: &str, agent: &str, now: i64) {
        assert!(store.claim_task(task_id, agent, 60, now).unwrap());
        assert!(store
            .record_conformance_and_transition(
                task_id,
                agent,
                ConformanceAudit {
                    evidence_schema_version: 1,
                    evidence: "{}",
                    verdict: Verdict::Aligned,
                    findings: "",
                },
                TaskStatus::Done,
                now,
            )
            .unwrap());
    }

    fn temporary_database(name: &str, iteration: usize) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "lodestar-{name}-{}-{iteration}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
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
    fn progressive_handoff_unblocks_successor_only_after_done_transition() {
        let store = store();
        let goal = goal(&store);
        let first = store
            .create_task(&goal.id, "Edit first symbol", "done", None, NOW)
            .unwrap();
        let second = store
            .create_task_after(
                &goal.id,
                "Edit second symbol",
                "done",
                None,
                Some(first.id.clone()),
                NOW + 1,
            )
            .unwrap();

        assert_eq!(second.status, TaskStatus::Blocked);
        assert_eq!(second.blocked_by.as_deref(), Some(first.id.as_str()));
        assert_eq!(store.next_task(NOW + 1).unwrap().unwrap().id, first.id);
        assert!(!store
            .claim_task(&second.id, "agent-b", 60, NOW + 1)
            .unwrap());

        assert!(store.claim_task(&first.id, "agent-a", 60, NOW + 1).unwrap());
        assert!(store
            .record_conformance_and_transition(
                &first.id,
                "agent-a",
                ConformanceAudit {
                    evidence_schema_version: 1,
                    evidence: "{}",
                    verdict: Verdict::Aligned,
                    findings: "",
                },
                TaskStatus::Done,
                NOW + 2,
            )
            .unwrap());

        let successor = store.get_task(&second.id).unwrap().unwrap();
        let predecessor = store.get_task(&first.id).unwrap().unwrap();
        assert!(predecessor.owner.is_none());
        assert!(predecessor.claim_started_at.is_none());
        assert!(predecessor.lease_expires_at.is_none());
        assert_eq!(successor.status, TaskStatus::Open);
        assert!(successor.blocked_by.is_none());
        assert_eq!(store.next_task(NOW + 2).unwrap().unwrap().id, second.id);
        assert!(store
            .claim_task(&second.id, "agent-b", 60, NOW + 2)
            .unwrap());
    }

    #[test]
    fn non_done_predecessor_transition_keeps_successor_blocked() {
        let store = store();
        let goal = goal(&store);
        let first = store
            .create_task(&goal.id, "Edit first symbol", "done", None, NOW)
            .unwrap();
        let second = store
            .create_task_after(
                &goal.id,
                "Edit second symbol",
                "done",
                None,
                Some(first.id.clone()),
                NOW + 1,
            )
            .unwrap();
        store.claim_task(&first.id, "agent-a", 60, NOW + 1).unwrap();

        assert!(store
            .record_conformance_and_transition(
                &first.id,
                "agent-a",
                ConformanceAudit {
                    evidence_schema_version: 1,
                    evidence: "{}",
                    verdict: Verdict::NeedsHuman,
                    findings: "review",
                },
                TaskStatus::InReview,
                NOW + 2,
            )
            .unwrap());

        assert_eq!(
            store.get_task(&second.id).unwrap().unwrap().status,
            TaskStatus::Blocked
        );
    }

    #[test]
    fn dependency_on_completed_task_starts_open_and_missing_task_is_rejected() {
        let store = store();
        let goal = goal(&store);
        let first = store
            .create_task(&goal.id, "Done predecessor", "done", None, NOW)
            .unwrap();
        store.claim_task(&first.id, "agent-a", 60, NOW).unwrap();
        store
            .record_conformance_and_transition(
                &first.id,
                "agent-a",
                ConformanceAudit {
                    evidence_schema_version: 1,
                    evidence: "{}",
                    verdict: Verdict::Aligned,
                    findings: "",
                },
                TaskStatus::Done,
                NOW + 1,
            )
            .unwrap();

        let ready = store
            .create_task_after(
                &goal.id,
                "Ready successor",
                "done",
                None,
                Some(first.id),
                NOW + 2,
            )
            .unwrap();
        assert_eq!(ready.status, TaskStatus::Open);
        assert!(ready.blocked_by.is_none());
        assert!(store
            .create_task_after(
                &goal.id,
                "Missing dependency",
                "done",
                None,
                Some("task:missing".to_string()),
                NOW + 3,
            )
            .is_err());
    }

    #[test]
    fn done_status_without_aligned_audit_does_not_satisfy_dependency() {
        let store = store();
        let goal = goal(&store);
        let predecessor = store
            .create_task(&goal.id, "Legacy done", "done", None, NOW)
            .unwrap();
        store
            .conn
            .execute(
                "UPDATE tasks SET status = 'done' WHERE id = ?1",
                params![predecessor.id],
            )
            .unwrap();

        let error = store
            .create_task_after(
                &goal.id,
                "Successor",
                "done",
                None,
                Some(predecessor.id),
                NOW + 1,
            )
            .unwrap_err();

        assert!(error.to_string().contains("without aligned completion"));
    }

    #[test]
    fn completed_predecessor_cannot_gain_a_second_successor() {
        let store = store();
        let goal = goal(&store);
        let first = store
            .create_task(&goal.id, "First", "done", None, NOW)
            .unwrap();
        let second = store
            .create_task_after(
                &goal.id,
                "Second",
                "done",
                None,
                Some(first.id.clone()),
                NOW + 1,
            )
            .unwrap();
        complete_aligned(&store, &first.id, "agent-a", NOW + 2);
        assert_eq!(
            store.get_task(&second.id).unwrap().unwrap().status,
            TaskStatus::Open
        );

        let error = store
            .create_task_after(&goal.id, "Third", "done", None, Some(first.id), NOW + 3)
            .unwrap_err();

        assert!(error.to_string().contains("already hands off"));
    }

    #[test]
    fn opened_successor_can_be_paused_without_erasing_handoff_lineage() {
        let store = store();
        let goal = goal(&store);
        let first = store
            .create_task(&goal.id, "First", "done", None, NOW)
            .unwrap();
        let second = store
            .create_task_after(
                &goal.id,
                "Second",
                "done",
                None,
                Some(first.id.clone()),
                NOW + 1,
            )
            .unwrap();
        complete_aligned(&store, &first.id, "agent-a", NOW + 2);
        store
            .claim_task(&second.id, "agent-b", 60, NOW + 3)
            .unwrap();

        assert!(store.block_task(&second.id, None, NOW + 4).unwrap());
        let paused = store.get_task(&second.id).unwrap().unwrap();
        assert_eq!(paused.status, TaskStatus::Blocked);
        assert!(paused.owner.is_none());
        assert!(paused.blocked_by.is_none());
        assert!(store
            .create_task_after(
                &goal.id,
                "Illegal fan-out",
                "done",
                None,
                Some(first.id),
                NOW + 5,
            )
            .is_err());
    }

    #[test]
    fn dependency_must_be_same_goal_linear_and_acyclic() {
        let store = store();
        let first_goal = goal(&store);
        let second_goal = store
            .define_goal(GoalKind::Objective, "Other goal", "Other work", None, NOW)
            .unwrap();
        let first = store
            .create_task(&first_goal.id, "First", "done", None, NOW)
            .unwrap();
        let second = store
            .create_task_after(
                &first_goal.id,
                "Second",
                "done",
                None,
                Some(first.id.clone()),
                NOW + 1,
            )
            .unwrap();

        assert!(store
            .create_task_after(
                &first_goal.id,
                "Fan out",
                "done",
                None,
                Some(first.id.clone()),
                NOW + 2,
            )
            .is_err());
        assert!(store
            .create_task_after(
                &second_goal.id,
                "Cross goal",
                "done",
                None,
                Some(first.id.clone()),
                NOW + 3,
            )
            .is_err());
        assert!(store
            .block_task(&first.id, Some(first.id.clone()), NOW + 4)
            .is_err());
        assert!(store
            .block_task(&first.id, Some(second.id.clone()), NOW + 4)
            .is_err());
    }

    #[test]
    fn blocking_a_claim_clears_owner_and_release_cannot_reopen_dependency() {
        let store = store();
        let goal = goal(&store);
        let predecessor = store
            .create_task(&goal.id, "Predecessor", "done", None, NOW)
            .unwrap();
        let task = store
            .create_task(&goal.id, "Current", "done", None, NOW + 1)
            .unwrap();
        assert!(store.claim_task(&task.id, "agent-a", 60, NOW + 1).unwrap());

        assert!(store
            .block_task(&task.id, Some(predecessor.id), NOW + 2)
            .unwrap());

        let blocked = store.get_task(&task.id).unwrap().unwrap();
        assert_eq!(blocked.status, TaskStatus::Blocked);
        assert!(blocked.owner.is_none());
        assert!(blocked.claim_started_at.is_none());
        assert!(blocked.lease_expires_at.is_none());
        assert!(!store.release_task(&task.id, "agent-a", NOW + 3).unwrap());
        assert!(!store.claim_task(&task.id, "agent-b", 60, NOW + 3).unwrap());
    }

    // Regression: before `reopen_task`, a task that landed in `in_review` (a
    // drift/needs-human completion) or was manually blocked with no predecessor
    // had no path back to a claimable state — it stranded until a manual DB edit.
    #[test]
    fn reopen_returns_stranded_tasks_to_claimable() {
        let store = store();
        let goal = goal(&store);

        // A manual hold (blocked with no predecessor) reopens and is claimable.
        let held = store
            .create_task(&goal.id, "Held", "done", None, NOW)
            .unwrap();
        assert!(store.block_task(&held.id, None, NOW + 1).unwrap());
        assert!(store.reopen_task(&held.id, NOW + 2).unwrap());
        let reopened = store.get_task(&held.id).unwrap().unwrap();
        assert_eq!(reopened.status, TaskStatus::Open);
        assert!(reopened.blocked_by.is_none());
        assert!(store.claim_task(&held.id, "agent-a", 60, NOW + 3).unwrap());

        // A drift outcome (in_review) reopens too.
        let review = store
            .create_task(&goal.id, "Review", "done", None, NOW)
            .unwrap();
        assert!(store.claim_task(&review.id, "agent-b", 60, NOW).unwrap());
        assert!(store
            .record_conformance_and_transition(
                &review.id,
                "agent-b",
                ConformanceAudit {
                    evidence_schema_version: 1,
                    evidence: "{}",
                    verdict: Verdict::Drift,
                    findings: "",
                },
                TaskStatus::InReview,
                NOW,
            )
            .unwrap());
        assert_eq!(
            store.get_task(&review.id).unwrap().unwrap().status,
            TaskStatus::InReview
        );
        assert!(store.reopen_task(&review.id, NOW + 1).unwrap());
        assert_eq!(
            store.get_task(&review.id).unwrap().unwrap().status,
            TaskStatus::Open
        );
    }

    #[test]
    fn reopen_refuses_gated_active_and_terminal_tasks() {
        let store = store();
        let goal = goal(&store);

        // A handoff-gated task must not be reopened around its predecessor.
        let predecessor = store
            .create_task(&goal.id, "Pred", "done", None, NOW)
            .unwrap();
        let gated = store
            .create_task_after(
                &goal.id,
                "Gated",
                "done",
                None,
                Some(predecessor.id.clone()),
                NOW + 1,
            )
            .unwrap();
        assert_eq!(gated.status, TaskStatus::Blocked);
        assert!(store
            .reopen_task(&gated.id, NOW + 2)
            .unwrap_err()
            .to_string()
            .contains("gated by predecessor"));

        // An open task is not stranded; a claimed task must be released instead.
        let open = store
            .create_task(&goal.id, "Open", "done", None, NOW)
            .unwrap();
        assert!(store
            .reopen_task(&open.id, NOW + 1)
            .unwrap_err()
            .to_string()
            .contains("cannot be reopened"));
        assert!(store.claim_task(&open.id, "agent-a", 60, NOW + 2).unwrap());
        assert!(store
            .reopen_task(&open.id, NOW + 3)
            .unwrap_err()
            .to_string()
            .contains("cannot be reopened"));

        // Terminal work stays terminal.
        let done = store
            .create_task(&goal.id, "Done", "done", None, NOW)
            .unwrap();
        complete_aligned(&store, &done.id, "agent-c", NOW);
        assert!(store
            .reopen_task(&done.id, NOW + 1)
            .unwrap_err()
            .to_string()
            .contains("cannot be reopened"));
    }

    #[test]
    fn store_rejects_verdict_status_mismatch() {
        let store = store();
        let goal = goal(&store);
        let task = store
            .create_task(&goal.id, "Task", "done", None, NOW)
            .unwrap();
        store.claim_task(&task.id, "agent-a", 60, NOW).unwrap();

        let error = store
            .record_conformance_and_transition(
                &task.id,
                "agent-a",
                ConformanceAudit {
                    evidence_schema_version: 1,
                    evidence: "{}",
                    verdict: Verdict::Aligned,
                    findings: "",
                },
                TaskStatus::InReview,
                NOW + 1,
            )
            .unwrap_err();

        assert!(error.to_string().contains("requires status done"));
        assert_eq!(
            store.get_task(&task.id).unwrap().unwrap().status,
            TaskStatus::Claimed
        );
    }

    #[test]
    fn three_task_chain_opens_only_one_successor_at_a_time() {
        let store = store();
        let goal = goal(&store);
        let first = store
            .create_task(&goal.id, "First", "done", None, NOW)
            .unwrap();
        let second = store
            .create_task_after(
                &goal.id,
                "Second",
                "done",
                None,
                Some(first.id.clone()),
                NOW + 1,
            )
            .unwrap();
        let third = store
            .create_task_after(
                &goal.id,
                "Third",
                "done",
                None,
                Some(second.id.clone()),
                NOW + 2,
            )
            .unwrap();

        complete_aligned(&store, &first.id, "agent-a", NOW + 3);
        assert_eq!(
            store.get_task(&second.id).unwrap().unwrap().status,
            TaskStatus::Open
        );
        assert_eq!(
            store.get_task(&third.id).unwrap().unwrap().status,
            TaskStatus::Blocked
        );
        complete_aligned(&store, &second.id, "agent-b", NOW + 4);
        assert_eq!(
            store.get_task(&third.id).unwrap().unwrap().status,
            TaskStatus::Open
        );
    }

    #[test]
    fn concurrent_completion_and_successor_creation_never_lose_the_handoff() {
        for iteration in 0..20 {
            let path = temporary_database("handoff-race", iteration);
            let setup = LodestarStore::new(db::open(path.to_str().unwrap()).unwrap());
            let goal = goal(&setup);
            let first = setup
                .create_task(&goal.id, "First", "done", None, NOW)
                .unwrap();
            setup.claim_task(&first.id, "agent-a", 60, NOW).unwrap();
            drop(setup);

            let barrier = Arc::new(Barrier::new(3));
            let create_path = path.clone();
            let create_goal = goal.id.clone();
            let create_first = first.id.clone();
            let create_barrier = barrier.clone();
            let creator = std::thread::spawn(move || {
                let store = LodestarStore::new(db::open(create_path.to_str().unwrap()).unwrap());
                create_barrier.wait();
                store
                    .create_task_after(
                        &create_goal,
                        "Second",
                        "done",
                        None,
                        Some(create_first),
                        NOW + 1,
                    )
                    .unwrap()
            });
            let complete_path = path.clone();
            let complete_first = first.id.clone();
            let complete_barrier = barrier.clone();
            let completer = std::thread::spawn(move || {
                let store = LodestarStore::new(db::open(complete_path.to_str().unwrap()).unwrap());
                complete_barrier.wait();
                store
                    .record_conformance_and_transition(
                        &complete_first,
                        "agent-a",
                        ConformanceAudit {
                            evidence_schema_version: 1,
                            evidence: "{}",
                            verdict: Verdict::Aligned,
                            findings: "",
                        },
                        TaskStatus::Done,
                        NOW + 1,
                    )
                    .unwrap()
            });
            barrier.wait();
            let successor = creator.join().unwrap();
            assert!(completer.join().unwrap());

            let inspector = LodestarStore::new(db::open(path.to_str().unwrap()).unwrap());
            let successor = inspector.get_task(&successor.id).unwrap().unwrap();
            assert_eq!(successor.status, TaskStatus::Open, "iteration {iteration}");
            assert!(successor.blocked_by.is_none());
            drop(inspector);
            std::fs::remove_file(path).unwrap();
        }
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
        s.block_task(&second.id, None, NOW).unwrap();
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

    // Regression: re-recording an existing statement silently kept the original
    // half-life (the ON CONFLICT clause updated weight/evidence/confirmed_at but
    // not half_life_hours), so a caller's revised revalidation cadence was lost.
    #[test]
    fn record_knowledge_updates_half_life_on_conflict() {
        let s = store();
        let first = s.record_knowledge("X breaks Y", "{}", 720.0, NOW).unwrap();
        assert_eq!(first.half_life_hours, 720.0);
        let second = s.record_knowledge("X breaks Y", "{}", 24.0, NOW).unwrap();
        assert_eq!(second.id, first.id);
        assert_eq!(second.half_life_hours, 24.0);
    }
}

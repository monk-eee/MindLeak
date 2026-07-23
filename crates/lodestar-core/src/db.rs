//! SQLite connection setup, schema application, and the `effective_weight`
//! scalar function used for knowledge revalidation queries.

use rusqlite::functions::FunctionFlags;
use rusqlite::{Connection, OptionalExtension};

use crate::error::{LodestarError, Result};

const SCHEMA: &str = include_str!("schema.sql");

/// Open (or create) a Lodestar database at `path` and configure it.
pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    Ok(conn)
}

/// Open an in-memory database (tests / ephemeral tooling).
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    // WAL + a generous busy timeout: many local agents/worktrees share one file
    // and race on the claim CAS; SQLite serialises writers, the timeout absorbs
    // contention instead of erroring.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    conn.execute_batch(SCHEMA)?;
    migrate(conn)?;
    register_functions(conn)?;
    Ok(())
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = migrate_locked(conn);
    match result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    Ok(())
}

fn migrate_locked(conn: &Connection) -> Result<()> {
    for (table, column, definition) in [
        ("tasks", "claim_started_at", "INTEGER"),
        ("goal_code", "mode", "TEXT NOT NULL DEFAULT 'governed'"),
        ("conformance", "evidence_schema_version", "INTEGER"),
        ("conformance", "evidence", "TEXT"),
    ] {
        if !column_exists(conn, table, column)? {
            conn.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition}"
            ))?;
        }
    }
    conn.execute(
        "UPDATE tasks
         SET claim_started_at = updated_at
         WHERE status = 'claimed' AND claim_started_at IS NULL",
        [],
    )?;
    let ambiguous: Option<String> = conn
        .query_row(
            "SELECT blocked_by
             FROM tasks
             WHERE blocked_by IS NOT NULL
             GROUP BY blocked_by
             HAVING COUNT(1) > 1
             LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(predecessor) = ambiguous {
        return Err(LodestarError::Invalid(format!(
            "legacy task {predecessor} has multiple successors; progressive handoff requires a linear chain"
        )));
    }
    let cross_goal: Option<(String, String)> = conn
        .query_row(
            "SELECT successor.id, predecessor.id
             FROM tasks successor
             JOIN tasks predecessor ON predecessor.id = successor.blocked_by
             WHERE successor.blocked_by IS NOT NULL
               AND successor.goal_id <> predecessor.goal_id
             LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    if let Some((successor, predecessor)) = cross_goal {
        return Err(LodestarError::Invalid(format!(
            "legacy handoff {predecessor} -> {successor} crosses goals"
        )));
    }
    let cycle: Option<String> = conn
        .query_row(
            "WITH RECURSIVE chain(start_id, id, path, cyclic) AS (
                 SELECT id, blocked_by, ',' || id || ',', 0
                 FROM tasks WHERE blocked_by IS NOT NULL
                 UNION ALL
                 SELECT chain.start_id, tasks.blocked_by,
                        chain.path || tasks.id || ',',
                        instr(chain.path, ',' || tasks.id || ',') > 0
                 FROM chain JOIN tasks ON tasks.id = chain.id
                 WHERE chain.id IS NOT NULL AND chain.cyclic = 0
             )
             SELECT start_id FROM chain WHERE cyclic = 1 LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(task) = cycle {
        return Err(LodestarError::Invalid(format!(
            "legacy task handoff containing {task} is cyclic"
        )));
    }
    conn.execute(
        "UPDATE tasks
         SET status = CASE
                 WHEN status IN ('open', 'claimed') THEN 'blocked'
                 ELSE status
             END,
             owner = NULL, claim_started_at = NULL,
             lease_expires_at = NULL
         WHERE blocked_by IS NOT NULL",
        [],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO task_handoffs
             (predecessor_id, successor_id, created_at)
         SELECT blocked_by, id, created_at
         FROM tasks
         WHERE blocked_by IS NOT NULL",
        [],
    )?;
    conn.execute(
        "UPDATE tasks
         SET status = 'open', owner = NULL, claim_started_at = NULL,
             lease_expires_at = NULL, blocked_by = NULL, updated_at = MAX(
                 updated_at,
                 COALESCE((
                     SELECT MAX(checked_at) FROM conformance
                     WHERE conformance.task_id = tasks.blocked_by
                       AND conformance.verdict = 'aligned'
                 ), updated_at)
             )
         WHERE status = 'blocked'
           AND EXISTS (
               SELECT 1 FROM tasks predecessor
               WHERE predecessor.id = tasks.blocked_by
                 AND predecessor.status = 'done'
           )
           AND EXISTS (
               SELECT 1 FROM conformance
               WHERE conformance.task_id = tasks.blocked_by
                 AND conformance.verdict = 'aligned'
           )",
        [],
    )?;
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn register_functions(conn: &Connection) -> Result<()> {
    conn.create_scalar_function(
        "effective_weight",
        4,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let base: f64 = ctx.get(0)?;
            let half_life: f64 = ctx.get(1)?;
            let confirmed_at: i64 = ctx.get(2)?;
            let now: i64 = ctx.get(3)?;
            Ok(crate::decay::effective_weight(
                base,
                half_life,
                confirmed_at,
                now,
            ))
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::model::{TaskStatus, Verdict};
    use crate::store::{ConformanceAudit, LodestarStore};

    #[test]
    fn migration_backfills_legacy_handoff_and_completion_opens_successor() {
        let path = temporary_database("legacy-handoff");
        create_legacy_database(&path, false);

        let store = LodestarStore::new(open(path.to_str().unwrap()).unwrap());
        assert!(store
            .record_conformance_and_transition(
                "task:first",
                "agent-a",
                ConformanceAudit {
                    evidence_schema_version: 1,
                    evidence: "{}",
                    verdict: Verdict::Aligned,
                    findings: "",
                },
                TaskStatus::Done,
                110,
            )
            .unwrap());

        let successor = store.get_task("task:second").unwrap().unwrap();
        assert_eq!(successor.status, TaskStatus::Open);
        assert!(successor.blocked_by.is_none());
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn migration_rejects_legacy_fan_out_without_partial_backfill() {
        let path = temporary_database("legacy-fanout");
        create_legacy_database(&path, true);

        let error = open(path.to_str().unwrap()).unwrap_err();
        assert!(error.to_string().contains("multiple successors"));
        let connection = Connection::open(&path).unwrap();
        let rows: i64 = connection
            .query_row("SELECT COUNT(1) FROM task_handoffs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(rows, 0);
        drop(connection);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn migration_rejects_legacy_cross_goal_and_cycles() {
        for (name, mutation, expected) in [
            (
                "legacy-cross-goal",
                "INSERT INTO goals
                     (id, slug, kind, title, statement, status, version, created_at)
                 VALUES ('goal:other', 'other', 'objective', 'Other', 'Other', 'active', 1, 1);
                 UPDATE tasks SET goal_id = 'goal:other' WHERE id = 'task:second';",
                "crosses goals",
            ),
            (
                "legacy-cycle",
                "UPDATE tasks SET blocked_by = 'task:second' WHERE id = 'task:first';",
                "cyclic",
            ),
        ] {
            let path = temporary_database(name);
            create_legacy_database(&path, false);
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch(mutation).unwrap();
            drop(connection);

            let error = open(path.to_str().unwrap()).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn migration_opens_legacy_successor_when_predecessor_already_completed_aligned() {
        let path = temporary_database("legacy-satisfied");
        create_legacy_database(&path, false);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "UPDATE tasks
                 SET status = 'done', owner = NULL, claim_started_at = NULL,
                     lease_expires_at = NULL
                 WHERE id = 'task:first';
                 INSERT INTO conformance
                     (task_id, evidence_schema_version, evidence, verdict, findings, checked_at)
                 VALUES ('task:first', 1, '{}', 'aligned', '', 105);",
            )
            .unwrap();
        drop(connection);

        let store = LodestarStore::new(open(path.to_str().unwrap()).unwrap());
        let successor = store.get_task("task:second").unwrap().unwrap();
        assert_eq!(successor.status, TaskStatus::Open);
        assert!(successor.blocked_by.is_none());
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn migration_normalizes_legacy_open_task_with_unresolved_dependency() {
        let path = temporary_database("legacy-open-blocked");
        create_legacy_database(&path, false);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "UPDATE tasks
                 SET status = 'open', owner = 'legacy-agent', claim_started_at = 1,
                     lease_expires_at = 999
                 WHERE id = 'task:second'",
                [],
            )
            .unwrap();
        drop(connection);

        let store = LodestarStore::new(open(path.to_str().unwrap()).unwrap());
        let successor = store.get_task("task:second").unwrap().unwrap();
        assert_eq!(successor.status, TaskStatus::Blocked);
        assert_eq!(successor.blocked_by.as_deref(), Some("task:first"));
        assert!(successor.owner.is_none());
        assert!(successor.claim_started_at.is_none());
        assert!(successor.lease_expires_at.is_none());
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn migration_preserves_nonclaimable_legacy_dependency_states() {
        for (iteration, (legacy, expected)) in [
            ("open", TaskStatus::Blocked),
            ("claimed", TaskStatus::Blocked),
            ("blocked", TaskStatus::Blocked),
            ("in_review", TaskStatus::InReview),
            ("done", TaskStatus::Done),
            ("abandoned", TaskStatus::Abandoned),
        ]
        .into_iter()
        .enumerate()
        {
            let path = temporary_database(&format!("legacy-status-{iteration}"));
            create_legacy_database(&path, false);
            let connection = Connection::open(&path).unwrap();
            connection
                .execute(
                    "UPDATE tasks
                     SET status = ?1, owner = 'legacy-agent', claim_started_at = 1,
                         lease_expires_at = 999
                     WHERE id = 'task:second'",
                    [legacy],
                )
                .unwrap();
            drop(connection);

            let store = LodestarStore::new(open(path.to_str().unwrap()).unwrap());
            let successor = store.get_task("task:second").unwrap().unwrap();
            assert_eq!(successor.status, expected, "legacy status {legacy}");
            assert_eq!(successor.blocked_by.as_deref(), Some("task:first"));
            assert!(successor.owner.is_none());
            assert!(successor.claim_started_at.is_none());
            assert!(successor.lease_expires_at.is_none());
            drop(store);
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn migration_opens_only_claimable_states_when_predecessor_is_satisfied() {
        for (iteration, (legacy, expected, dependency_cleared)) in [
            ("open", TaskStatus::Open, true),
            ("claimed", TaskStatus::Open, true),
            ("blocked", TaskStatus::Open, true),
            ("in_review", TaskStatus::InReview, false),
            ("done", TaskStatus::Done, false),
            ("abandoned", TaskStatus::Abandoned, false),
        ]
        .into_iter()
        .enumerate()
        {
            let path = temporary_database(&format!("legacy-satisfied-status-{iteration}"));
            create_legacy_database(&path, false);
            let connection = Connection::open(&path).unwrap();
            connection
                .execute(
                    "UPDATE tasks
                     SET status = ?1, owner = 'legacy-agent', claim_started_at = 1,
                         lease_expires_at = 999
                     WHERE id = 'task:second'",
                    [legacy],
                )
                .unwrap();
            connection
                .execute_batch(
                    "UPDATE tasks
                     SET status = 'done', owner = NULL, claim_started_at = NULL,
                         lease_expires_at = NULL
                     WHERE id = 'task:first';
                     INSERT INTO conformance
                         (task_id, evidence_schema_version, evidence, verdict, findings, checked_at)
                     VALUES ('task:first', 1, '{}', 'aligned', '', 105);",
                )
                .unwrap();
            drop(connection);

            let store = LodestarStore::new(open(path.to_str().unwrap()).unwrap());
            let successor = store.get_task("task:second").unwrap().unwrap();
            assert_eq!(successor.status, expected, "legacy status {legacy}");
            assert_eq!(successor.blocked_by.is_none(), dependency_cleared);
            assert!(successor.owner.is_none());
            assert!(successor.claim_started_at.is_none());
            assert!(successor.lease_expires_at.is_none());
            drop(store);
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn migration_opens_legacy_open_task_when_dependency_is_already_satisfied() {
        let path = temporary_database("legacy-open-satisfied");
        create_legacy_database(&path, false);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "UPDATE tasks
                 SET status = 'done', owner = NULL, claim_started_at = NULL,
                     lease_expires_at = NULL
                 WHERE id = 'task:first';
                 UPDATE tasks SET status = 'open' WHERE id = 'task:second';
                 INSERT INTO conformance
                     (task_id, evidence_schema_version, evidence, verdict, findings, checked_at)
                 VALUES ('task:first', 1, '{}', 'aligned', '', 105);",
            )
            .unwrap();
        drop(connection);

        let store = LodestarStore::new(open(path.to_str().unwrap()).unwrap());
        let successor = store.get_task("task:second").unwrap().unwrap();
        assert_eq!(successor.status, TaskStatus::Open);
        assert!(successor.blocked_by.is_none());
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    fn create_legacy_database(path: &PathBuf, fan_out: bool) {
        let connection = Connection::open(path).unwrap();
        connection.execute_batch(SCHEMA).unwrap();
        connection
            .execute(
                "INSERT INTO goals
                     (id, slug, kind, title, statement, status, version, created_at)
                 VALUES ('goal:test', 'test', 'objective', 'Test', 'Test', 'active', 1, 1)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO tasks
                     (id, goal_id, title, acceptance, status, owner, claim_started_at,
                      lease_expires_at, blocked_by, created_at, updated_at)
                 VALUES ('task:first', 'goal:test', 'First', '', 'claimed', 'agent-a',
                         100, 200, NULL, 1, 100)",
                [],
            )
            .unwrap();
        for id in if fan_out {
            vec!["task:second", "task:third"]
        } else {
            vec!["task:second"]
        } {
            connection
                .execute(
                    "INSERT INTO tasks
                         (id, goal_id, title, acceptance, status, blocked_by, created_at, updated_at)
                     VALUES (?1, 'goal:test', ?1, '', 'blocked', 'task:first', 2, 2)",
                    [id],
                )
                .unwrap();
        }
        connection.execute("DELETE FROM task_handoffs", []).unwrap();
    }

    fn temporary_database(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("lodestar-db-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }
}

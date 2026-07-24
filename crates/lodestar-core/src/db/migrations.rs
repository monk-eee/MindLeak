//! Transactional schema migration for existing Lodestar databases.

use rusqlite::{Connection, OptionalExtension};

use crate::error::{LodestarError, Result};

pub(super) fn migrate(connection: &Connection) -> Result<()> {
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = migrate_locked(connection);
    match result {
        Ok(()) => connection.execute_batch("COMMIT")?,
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            return Err(error);
        }
    }
    Ok(())
}

fn migrate_locked(connection: &Connection) -> Result<()> {
    let promotion_status_added = !column_exists(connection, "design_items", "promotion_status")?;
    for (table, column, definition) in [
        ("tasks", "claim_started_at", "INTEGER"),
        ("tasks", "parked_at", "INTEGER"),
        ("goal_code", "mode", "TEXT NOT NULL DEFAULT 'governed'"),
        ("conformance", "evidence_schema_version", "INTEGER"),
        ("conformance", "evidence", "TEXT"),
        (
            "design_items",
            "promotion_status",
            "TEXT NOT NULL DEFAULT 'not_required'",
        ),
        (
            "design_items",
            "materialization_revision",
            "INTEGER NOT NULL DEFAULT 0",
        ),
    ] {
        if !column_exists(connection, table, column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition}"
            ))?;
        }
    }
    connection.execute(
        "UPDATE tasks
         SET claim_started_at = updated_at
         WHERE status = 'claimed' AND claim_started_at IS NULL",
        [],
    )?;
    if promotion_status_added {
        let has_legacy_goal = column_exists(connection, "design_items", "spawned_goal_id")?;
        let accepted_status = if has_legacy_goal {
            "CASE WHEN spawned_goal_id IS NOT NULL THEN 'materialized' ELSE 'pending' END"
        } else {
            "'pending'"
        };
        connection.execute_batch(&format!(
            "UPDATE design_items
             SET promotion_status = CASE
                 WHEN status = 'accepted' THEN {accepted_status}
                 ELSE 'not_required'
             END"
        ))?;
    }
    connection.execute(
        "INSERT OR IGNORE INTO design_materializations
             (design_id, revision, mode, plan_json, rationale, actor, created_at)
         SELECT id, 1, 'create',
                '{\"mode\":\"create\",\"tasks\":[],\"task_ids\":[],\"constraints\":[],\"rationale\":\"Migrated legacy materialization\"}',
                'Migrated legacy materialization',
                COALESCE(decided_by, 'migration'), updated_at
         FROM design_items
         WHERE promotion_status = 'materialized' AND materialization_revision = 0",
        [],
    )?;
    connection.execute(
        "UPDATE design_items SET materialization_revision = 1
         WHERE promotion_status = 'materialized' AND materialization_revision = 0",
        [],
    )?;
    connection.execute(
        "UPDATE tasks
         SET status = CASE
                 WHEN status IN ('open', 'claimed') THEN 'blocked'
                 ELSE status
             END,
             owner = NULL, claim_started_at = NULL, lease_expires_at = NULL,
             blocked_by = NULL
         WHERE blocked_by IS NOT NULL
           AND NOT EXISTS (
               SELECT 1 FROM tasks predecessor
               WHERE predecessor.id = tasks.blocked_by
           )",
        [],
    )?;
    let ambiguous: Option<String> = connection
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
    let cross_goal: Option<(String, String)> = connection
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
    let cycle: Option<String> = connection
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
    connection.execute(
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
    connection.execute(
        "INSERT OR IGNORE INTO task_handoffs
             (predecessor_id, successor_id, created_at)
         SELECT blocked_by, id, created_at
         FROM tasks
         WHERE blocked_by IS NOT NULL",
        [],
    )?;
    connection.execute(
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

fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

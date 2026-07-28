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
        ("tasks", "claim_lapses", "INTEGER NOT NULL DEFAULT 0"),
        ("tasks", "unleased_seconds", "INTEGER NOT NULL DEFAULT 0"),
        // Who overrode a non-affirming verdict, when, and which verdict
        // (ADR-0009). NULL on every pre-existing row is the honest answer:
        // those acceptances were not recorded and cannot be reconstructed.
        ("tasks", "resolved_by", "TEXT"),
        ("tasks", "resolved_at", "INTEGER"),
        ("tasks", "resolved_conformance_id", "INTEGER"),
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
        ("design_items", "retired_at", "INTEGER"),
        ("design_items", "retired_by", "TEXT"),
        ("design_items", "retired_reason", "TEXT"),
        ("design_items", "superseded_by", "TEXT"),
        ("design_items", "superseded_at", "INTEGER"),
        ("design_items", "superseded_by_human", "TEXT"),
        ("goals", "constitution_version", "TEXT"),
        ("goals", "rationale", "TEXT"),
        ("goals", "scope", "TEXT"),
        ("goals", "evidence_contract", "TEXT"),
        ("goals", "consequence", "TEXT"),
        ("goals", "waivable", "INTEGER NOT NULL DEFAULT 0"),
        ("goals", "waiver_authority", "TEXT"),
        ("goals", "origin", "TEXT NOT NULL DEFAULT 'local'"),
        ("task_qa", "audience", "TEXT"),
    ] {
        if !column_exists(connection, table, column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition}"
            ))?;
        }
    }
    migrate_constitution_versions(connection)?;
    collapse_session_identities(connection)?;
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

/// Collapse `session:v1:{name}:{fingerprint}` identities to `session:v1:{fingerprint}` (ADR-0054).
///
/// The old id embedded a label read from the *hosting process* environment
/// (`LODESTAR_AGENT`), while the fingerprint came from the session token. One
/// agent whose server started twice with different environments therefore held
/// two identities. Every comparison in the system is whole-string equality —
/// `tasks.owner`, `task_qa.audience`, overlap, wait cycles — so the fleet saw
/// two agents where there was one, and a question addressed to one half was
/// invisible to the other.
///
/// The rewrite heals that automatically: both halves share the fingerprint, so
/// both collapse onto the same id and the split agent becomes one agent with
/// all of its claims. `substr(value, -32)` takes the fingerprint, which is
/// always exactly 32 hex characters; the `GLOB` matches only ids that still
/// carry a label, so this is idempotent and a second open rewrites nothing.
fn collapse_session_identities(connection: &Connection) -> Result<()> {
    // `session_context` is keyed on the identity, so two halves of one agent
    // collapse onto the same primary key. Keep the most recent declaration and
    // drop the rest before rewriting, or the UPDATE fails on the constraint.
    if column_exists(connection, "session_context", "agent_id")? {
        connection.execute(
            "DELETE FROM session_context
             WHERE rowid NOT IN (
                 SELECT rowid FROM (
                     SELECT rowid,
                            ROW_NUMBER() OVER (
                                PARTITION BY substr(agent_id, -32)
                                ORDER BY declared_at DESC, rowid DESC
                            ) AS rank
                     FROM session_context
                     WHERE agent_id LIKE 'session:v1:%'
                 )
                 WHERE rank = 1
             )
             AND agent_id LIKE 'session:v1:%'",
            [],
        )?;
    }
    for (table, column) in [
        ("constitution_versions", "created_by"),
        ("constitution_versions", "activated_by"),
        ("waivers", "approved_by"),
        ("waivers", "revoked_by"),
        ("constitution_amendments", "amended_by"),
        ("design_items", "proposed_by"),
        ("design_items", "decided_by"),
        ("design_items", "retired_by"),
        ("design_materializations", "actor"),
        ("tasks", "owner"),
        ("task_claim_transfers", "from_owner"),
        ("task_claim_transfers", "to_owner"),
        ("task_claim_transfers", "recovered_by"),
        ("task_qa", "author"),
        ("task_qa", "audience"),
        ("session_context", "agent_id"),
    ] {
        if !column_exists(connection, table, column)? {
            continue;
        }
        connection.execute(
            &format!(
                "UPDATE {table}
                 SET {column} = 'session:v1:' || substr({column}, -32)
                 WHERE {column} GLOB 'session:v1:*:*'"
            ),
            [],
        )?;
    }
    Ok(())
}

/// Freeze the existing local goals as the first constitutional version.
///
/// The goals ARE today's active constitution, so they become version 1 with
/// honest provenance: `origin=local` (column default) and `created_by=migration`.
/// Migration does NOT invent a purpose, preamble, project identity, consequence,
/// or waiver policy (SPEC-CONSTITUTION §10, ADR-0026); those fields stay NULL so
/// incomplete clauses remain review-only until a maintainer completes them. The
/// guards make this idempotent: a second open adds no new version and rebinds no
/// clause.
fn migrate_constitution_versions(connection: &Connection) -> Result<()> {
    let has_goals: bool =
        connection.query_row("SELECT EXISTS(SELECT 1 FROM goals)", [], |row| row.get(0))?;
    let has_version: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM constitution_versions)",
        [],
        |row| row.get(0),
    )?;
    if has_goals && !has_version {
        let created_at: i64 = connection.query_row(
            "SELECT COALESCE(MIN(created_at), 0) FROM goals",
            [],
            |row| row.get(0),
        )?;
        connection.execute(
            "INSERT INTO constitution_versions
                 (id, version, project_identity, purpose, preamble, status,
                  created_by, created_at, activated_by, activated_at)
             VALUES ('constitution:v1', 1, NULL, NULL, NULL, 'active',
                     'migration', ?1, 'migration', ?1)",
            [created_at],
        )?;
    }
    connection.execute(
        "UPDATE goals SET constitution_version = 'constitution:v1'
         WHERE constitution_version IS NULL
           AND EXISTS (SELECT 1 FROM constitution_versions WHERE id = 'constitution:v1')",
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

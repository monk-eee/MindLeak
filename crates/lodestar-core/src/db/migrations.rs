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
        // Who stood a control down, and when (ADR-0034). NULL on every
        // pre-existing row is the honest answer: those retirements were not
        // recorded and cannot be reconstructed.
        ("controls", "retired_by", "TEXT"),
        ("controls", "retired_at", "INTEGER"),
        ("task_qa", "audience", "TEXT"),
    ] {
        if !column_exists(connection, table, column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition}"
            ))?;
        }
    }
    migrate_constitution_versions(connection)?;
    run_once(connection, "collapse_session_identities", || {
        collapse_session_identities(connection)
    })?;
    run_once(connection, "reconnect_superseded_clauses", || {
        reconnect_superseded_clauses(connection)
    })?;
    // Seed the task log with the present (ADR-0064). Recorded by name rather
    // than guarded by pattern, per ADR-0063 decision 3: "has this already
    // happened?" must be a fact, not an inference from the data the migration
    // itself is editing. This appends only — no task row is touched, so no
    // live claim moves.
    run_once(connection, "import_task_genesis_events", || {
        crate::store::import_genesis(connection, crate::now_unix()).map(|_| ())
    })?;
    // Only after the genesis has carried them into the log (ADR-0064 d5/d6).
    // Order is load-bearing: the counters are the sole surviving trace of a
    // window that opened before the log, so dropping them first would lose the
    // holes and let a discontinuous window certify itself clean.
    //
    // DROP COLUMN, never a table rebuild. A rebuild rewrites every row,
    // including `owner` on live claims, and ADR-0063 is explicit that a live
    // claim is not ours to touch. Dropping an unrelated column moves nothing.
    run_once(connection, "drop_task_lapse_counters", || {
        for column in ["claim_lapses", "unleased_seconds"] {
            if column_exists(connection, "tasks", column)? {
                connection.execute_batch(&format!("ALTER TABLE tasks DROP COLUMN {column}"))?;
            }
        }
        Ok(())
    })?;
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

/// Run a migration at most once per database, recorded by name (ADR-0063).
///
/// Pattern-idempotence — "rewrite every row that still looks unmigrated" — is
/// only idempotent while nothing else creates such rows. A rewrite that races a
/// live writer re-fires on every open, forever, and each firing looks exactly
/// like the first. That is not a theoretical hazard: it re-owned a live claim
/// out from under its holder every time any process opened the database, which
/// is how a task ended up provable by nobody. Anything touching identity or
/// ownership belongs here rather than in the pattern-guarded loop above.
fn run_once(
    connection: &Connection,
    name: &str,
    migration: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let applied: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM schema_migrations WHERE name = ?1",
            [name],
            |row| row.get(0),
        )
        .optional()?;
    if applied.is_some() {
        return Ok(());
    }
    migration()?;
    connection.execute(
        "INSERT INTO schema_migrations (name, applied_at) VALUES (?1, ?2)",
        rusqlite::params![name, crate::now_unix()],
    )?;
    Ok(())
}

/// Reconnect clauses stranded by an amendment that did not record where they went.
///
/// `amend_constitution` used to supersede the outgoing clauses with a bare
/// status flip, leaving `superseded_by` NULL. Because an amendment renames
/// every clause it carries forward (`goal:{slug}@{version}`), nothing could
/// follow the rename, so code bindings and open tasks kept naming clauses no
/// active constitution contained.
///
/// Measured on this repository after `constitution:v2` was adopted: 25 active
/// clauses held zero bindings and zero tasks, while all 156 bindings and all
/// 217 tasks named superseded v1 ids. The symptom was silent and read as
/// health — `governing_goals` filters to active clauses, so it reported
/// "nothing governs this" for files that were bound, and `advise` answered
/// "no active clause governs this change; proceed" for every change.
///
/// Repairs that state with the same same-slug rule the fixed amendment uses,
/// and moves bindings and live work together, because moving either alone is
/// what turns a silent gap into a fleet-wide drift report.
///
/// A task that is `claimed` with an unexpired lease is left alone (ADR-0063).
/// Its goal is what conformance judges the holder's evidence against, so moving
/// it mid-claim would change the rule under someone doing the work — the same
/// class of harm as rewriting `tasks.owner`, and not ours to do as a side
/// effect of opening a file. Those tasks heal at the next amendment instead,
/// which is an attributed act. Measured here: 3 of 56.
fn reconnect_superseded_clauses(connection: &Connection) -> Result<()> {
    connection.execute(
        "UPDATE goals AS outgoing
            SET superseded_by = (
                SELECT successor.id FROM goals AS successor
                 WHERE successor.status = 'active'
                   AND successor.slug = outgoing.slug
            )
          WHERE outgoing.status = 'superseded'
            AND outgoing.superseded_by IS NULL
            AND (SELECT COUNT(*) FROM goals AS successor
                  WHERE successor.status = 'active'
                    AND successor.slug = outgoing.slug) = 1",
        [],
    )?;
    connection.execute(
        "UPDATE OR REPLACE goal_code
            SET goal_id = (
                SELECT outgoing.superseded_by FROM goals AS outgoing
                 WHERE outgoing.id = goal_code.goal_id
            )
          WHERE goal_id IN (
                SELECT id FROM goals
                 WHERE status = 'superseded' AND superseded_by IS NOT NULL
            )",
        [],
    )?;
    connection.execute(
        "UPDATE tasks
            SET goal_id = (
                SELECT outgoing.superseded_by FROM goals AS outgoing
                 WHERE outgoing.id = tasks.goal_id
            )
          WHERE status NOT IN ('done', 'abandoned')
            AND NOT (status = 'claimed' AND lease_expires_at > ?1)
            AND goal_id IN (
                SELECT id FROM goals
                 WHERE status = 'superseded' AND superseded_by IS NOT NULL
            )",
        [crate::now_unix()],
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
    // `tasks.owner` is deliberately not in that list. Every other column above
    // is a historical record, and rewriting one changes only how the past reads.
    // `owner` is *live state*: it is what `check_conformance`, `ask_question`,
    // `renew_lease` and `complete_task` compare the caller against, so editing
    // it mid-claim does not adjust a record, it transfers ownership. An agent
    // whose id no longer matches cannot prove its work, cannot park the task to
    // explain, and reads as a different owner on re-claim — which opens a fresh
    // evidence window and orphans everything it had already committed.
    //
    // A claim that is over is safe to tidy; a claim that is live is not ours to
    // touch. Ownership only changes by claim, release, or an audited transfer.
    if column_exists(connection, "tasks", "owner")? {
        connection.execute(
            "UPDATE tasks
             SET owner = 'session:v1:' || substr(owner, -32)
             WHERE owner GLOB 'session:v1:*:*'
               AND NOT (status = 'claimed' AND COALESCE(lease_expires_at, 0) >= ?1)",
            [crate::now_unix()],
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

pub(crate) fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

//! Transactional schema migration for existing Lodestar databases.

mod support;

use rusqlite::{Connection, OptionalExtension};

use crate::error::{LodestarError, Result};

use support::run_once;
pub(crate) use support::{column_exists, table_exists};

pub(super) fn migrate(connection: &Connection) -> Result<bool> {
    connection.execute_batch("BEGIN IMMEDIATE")?;
    let result = migrate_locked(connection);
    match result {
        Ok(rebuilt) => {
            connection.execute_batch("COMMIT")?;
            Ok(rebuilt)
        }
        Err(error) => {
            let _ = connection.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

/// Reports whether a step rebuilt a table, so the caller can reclaim its pages.
fn migrate_locked(connection: &Connection) -> Result<bool> {
    // ADR-0060 renamed the binding verb to link_goal_to_artifact; the store it
    // writes to follows. schema.sql has already ensured an (empty)
    // goal_artifacts exists, so for an existing ledger this moves every binding
    // across from the old goal_code and drops it — a pure RENAME would strand the
    // real rows behind the empty table schema.sql just created. It runs before
    // the column loop below refers to the table by its new name, and adds the
    // `mode` column to a pre-mode goal_code first so the copy cannot fail.
    // Recorded by name (ADR-0063): "has this run?" is a fact, not an inference
    // from the rows it edits. Dropping goal_code drops its index with it.
    run_once(connection, "rename_goal_code_to_goal_artifacts", || {
        if table_exists(connection, "goal_code")? {
            if !column_exists(connection, "goal_code", "mode")? {
                connection.execute_batch(
                    "ALTER TABLE goal_code ADD COLUMN mode TEXT NOT NULL DEFAULT 'governed'",
                )?;
            }
            connection.execute_batch(
                "INSERT OR IGNORE INTO goal_artifacts (goal_id, node_id, mode)
                     SELECT goal_id, node_id, mode FROM goal_code;
                 DROP TABLE goal_code;",
            )?;
        }
        Ok(())
    })?;
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
        ("goal_artifacts", "mode", "TEXT NOT NULL DEFAULT 'governed'"),
        ("conformance", "evidence_schema_version", "INTEGER"),
        ("conformance", "evidence", "TEXT"),
        // Pre-existing audits retain their original text only. The exact vector
        // boundaries were never stored, so migration must not invent them.
        ("conformance", "findings_json", "TEXT"),
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
        ("design_items", "deferred_at", "INTEGER"),
        ("design_items", "deferred_by", "TEXT"),
        ("design_items", "deferred_reason", "TEXT"),
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
        ("goals", "source_system", "TEXT"),
        ("goals", "external_id", "TEXT"),
        ("goals", "source_ref", "TEXT"),
        ("goals", "source_digest", "TEXT"),
        // Who superseded a clause, and when (ADR-0142). NULL on every
        // pre-existing row is the honest answer: those supersessions recorded
        // only a free-form reason, and naming any agent as the one who
        // performed them would invent an attribution the ledger never held.
        // `ledger_act_evidence` refuses a NULL rather than guessing, which is
        // exactly why the column is nullable instead of defaulted.
        ("goals", "superseded_at", "INTEGER"),
        ("goals", "superseded_by_agent", "TEXT"),
        // Who stood a control down, and when (ADR-0034). NULL on every
        // pre-existing row is the honest answer: those retirements were not
        // recorded and cannot be reconstructed.
        ("controls", "retired_by", "TEXT"),
        ("controls", "retired_at", "INTEGER"),
        ("task_qa", "audience", "TEXT"),
        // A lesson can be retired rather than waited out (ADR-0019 shape, as
        // design_items above). NULL on every pre-existing row is the honest
        // answer: nothing was retired before this existed.
        ("knowledge", "retired_at", "INTEGER"),
        ("knowledge", "retired_by", "TEXT"),
        ("knowledge", "retired_reason", "TEXT"),
        ("knowledge", "superseded_by", "TEXT"),
        // ADR-0057. Backfills as NULL, which is the honest answer: the branch a
        // task was claimed on before this existed was never recorded, and
        // inferring one from where the agent happens to be now would invent a
        // fact about the past.
        // ADR-0043. Backfills as NULL, which is the honest answer: amendments
        // recorded before this existed named only the agent that executed them,
        // and naming that agent as its own approver would assert exactly the
        // separation this column exists to prove.
        ("constitution_amendments", "approved_by", "TEXT"),
        ("tasks", "branch", "TEXT"),
    ] {
        if !column_exists(connection, table, column)? {
            connection.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition}"
            ))?;
        }
    }
    migrate_constitution_versions(connection)?;
    run_once(connection, "collapse_session_identities", || {
        super::repairs::collapse_session_identities(connection)
    })?;
    run_once(connection, "reconnect_superseded_clauses", || {
        super::repairs::reconnect_superseded_clauses(connection)
    })?;
    run_once(
        connection,
        "clear_backfilled_constitution_version_from_objectives",
        || super::repairs::clear_backfilled_constitution_version_from_objectives(connection),
    )?;
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
    // `knowledge_embeddings` is built lazily by the optional index pass in
    // `embed`, away from schema.sql, and so was written without the cascade
    // `knowledge_sources` already carries onto the same parent. A decay prune
    // (`store::knowledge`) or a lifecycle clear therefore left a vector behind
    // describing a lesson that no longer exists. The Memory Plane shipped this
    // identical defect and reached 48,502 orphans and 142.1 MiB before anyone
    // looked; here it was still 0, which is the only reason this is cheap.
    //
    // A rebuild is safe for this table where the note above forbids one for
    // `tasks`: it carries no identity, ownership, or claim that a rewrite could
    // disturb -- only a derived, regenerable vector keyed by the lesson it
    // describes, copied across verbatim.
    let rebuilt = std::cell::Cell::new(false);
    run_once(
        connection,
        "cascade_knowledge_embeddings_onto_knowledge",
        || {
            rebuilt.set(cascade_knowledge_embeddings(connection)?);
            Ok(())
        },
    )?;
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
    Ok(rebuilt.get())
}

/// Give `knowledge_embeddings` the cascade onto `knowledge` that every other
/// child of it already has.
///
/// Copying only rows whose lesson still exists is what discards any orphan, so
/// the purge is a consequence of the copy rather than a second statement that
/// could disagree with it. Renaming first lets `ensure_table` build the
/// replacement, so the rebuilt table is by construction the one production
/// creates rather than a second copy of the definition that could drift.
fn cascade_knowledge_embeddings(connection: &Connection) -> Result<bool> {
    // Absent until something indexes, and `ensure_table` now builds it correctly.
    if !table_exists(connection, "knowledge_embeddings")? {
        return Ok(false);
    }
    connection.execute_batch(
        "ALTER TABLE knowledge_embeddings RENAME TO knowledge_embeddings_superseded;",
    )?;
    crate::embed::ensure_table(connection)?;
    connection.execute_batch(
        "INSERT INTO knowledge_embeddings (knowledge_id, model, dim, vector, updated_at)
         SELECT e.knowledge_id, e.model, e.dim, e.vector, e.updated_at
         FROM knowledge_embeddings_superseded e
         JOIN knowledge k ON k.id = e.knowledge_id;
         DROP TABLE knowledge_embeddings_superseded;",
    )?;
    Ok(true)
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
        // Freeze only the goals that already existed at this one-time
        // transition. Run unconditionally, this would re-fire on every
        // subsequent open and mislabel any goal created afterwards (a plain
        // `define_goal` objective always inserts a NULL constitution_version)
        // as belonging to a version the constitution may have long since
        // superseded.
        connection.execute(
            "UPDATE goals SET constitution_version = 'constitution:v1'
             WHERE constitution_version IS NULL",
            [],
        )?;
    }
    Ok(())
}

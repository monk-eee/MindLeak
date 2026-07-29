//! One-shot data repairs run by the migration engine.
//!
//! These are not schema evolution. Each one reaches into rows that a past
//! defect wrote wrongly and puts them right, once, under the `run_once` guard
//! in [`super::migrations`] — a rewrite that re-fires on every open is how a
//! live claim got re-owned out from under its holder.
//!
//! They live apart from the engine because they accumulate: every defect that
//! reaches production adds one, while the engine that runs them does not grow.
//! Keeping them together pushed the module past the size the constitution
//! allows, and the two have different reasons to change.

use rusqlite::Connection;

use crate::error::Result;

use super::migrations::column_exists;

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
pub(super) fn reconnect_superseded_clauses(connection: &Connection) -> Result<()> {
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
pub(super) fn collapse_session_identities(connection: &Connection) -> Result<()> {
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
pub(super) fn migrate_constitution_versions(connection: &Connection) -> Result<()> {
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

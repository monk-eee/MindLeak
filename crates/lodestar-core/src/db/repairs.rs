//! One-off data repairs, separate from schema migration.
//!
//! A schema migration adds a column or a table: it is about *shape*, it is
//! cheap to re-run, and it is guarded by asking whether the column exists. A
//! repair rewrites *rows* to undo damage a defect already did. The two are
//! easy to file together and should not be, because they fail differently — a
//! repair that fires twice can undo work someone did in between, so every one
//! here runs under `run_once` and is recorded in `schema_migrations`.
//!
//! Keeping them apart also keeps `migrations.rs` inside the module-length
//! clause. That is not the reason for the split, but it is the reason it
//! happened when it did: adding the repair below pushed that file from roughly
//! 416 to 476 non-test lines, past the 450 the clause allows.

use rusqlite::Connection;

use crate::error::Result;

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
        "UPDATE OR REPLACE goal_artifacts
            SET goal_id = (
                SELECT outgoing.superseded_by FROM goals AS outgoing
                 WHERE outgoing.id = goal_artifacts.goal_id
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

/// Undo the wrong tag left by the one-time freeze's backfill firing on every
/// open instead of only the one that creates `constitution:v1`.
///
/// `define_goal` always inserts a NULL `constitution_version` (a plain
/// objective is not part of the versioned clause set an amendment carries
/// forward). `migrate_constitution_versions`' backfill nonetheless matched
/// any NULL row unconditionally, so an objective created long after the
/// constitution moved past v1 was silently mislabeled the next time the
/// database was merely reopened. Measured live in this repository: three
/// objectives (compile_context, compile_digest, and a superseded-in-error
/// duplicate of the title_twin work) carried `constitution:v1` while the
/// active version was already `constitution:v4`.
///
/// Named by exact id rather than a general "objective with a stale tag"
/// rule: a broader predicate risks reaching a goal an amendment genuinely
/// carried forward, which this repair must never touch.
pub(super) fn clear_backfilled_constitution_version_from_objectives(
    connection: &Connection,
) -> Result<()> {
    connection.execute(
        "UPDATE goals SET constitution_version = NULL
          WHERE id IN (
              'goal:adr-0102-dynamic-context-compiler-compile-contex',
              'goal:adr-0101-digest-compilation-compile-digest',
              'goal:adr-0099-claim-also-checks-for-a-live-twin-by-ti-v2'
          )
            AND kind = 'objective'
            AND constitution_version = 'constitution:v1'",
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
///
/// Moved here from `migrations.rs` (see that module's own header): this is a
/// row-level repair of identities a past defect already split, not a schema
/// shape change, so it belongs beside the other repairs rather than in the
/// migration that runs it.
pub(super) fn collapse_session_identities(connection: &Connection) -> Result<()> {
    // `session_context` is keyed on the identity, so two halves of one agent
    // collapse onto the same primary key. Keep the most recent declaration and
    // drop the rest before rewriting, or the UPDATE fails on the constraint.
    if super::migrations::column_exists(connection, "session_context", "agent_id")? {
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
        if !super::migrations::column_exists(connection, table, column)? {
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
    // A claim that is over is safe to tidy; a claim that is live or deliberately
    // parked is not ours to touch. A parked task retains both its owner and
    // evidence window specifically so that owner can resume it. Ownership only
    // changes by claim, release, or an audited transfer.
    if super::migrations::column_exists(connection, "tasks", "owner")? {
        connection.execute(
            "UPDATE tasks
             SET owner = 'session:v1:' || substr(owner, -32)
             WHERE owner GLOB 'session:v1:*:*'
               AND NOT (
                   (status = 'claimed' AND COALESCE(lease_expires_at, 0) >= ?1)
                   OR status IN ('needs_input', 'paused')
               )",
            [crate::now_unix()],
        )?;
    }
    Ok(())
}

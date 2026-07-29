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

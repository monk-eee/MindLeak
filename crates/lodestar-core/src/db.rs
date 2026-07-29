//! SQLite connection setup, schema application, and the `effective_weight`
//! scalar function used for knowledge revalidation queries.

mod functions;
mod migrations;

use rusqlite::Connection;

use crate::error::Result;

const SCHEMA: &str = include_str!("schema.sql");
/// Indexes are applied *after* migrations — see the header of `indexes.sql`.
const INDEXES: &str = include_str!("indexes.sql");

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
    // Order is load-bearing: an index over a column a migration adds cannot be
    // created before that migration runs. On an existing database the CREATE
    // TABLE statements are no-ops, so the pre-migration table shape is what the
    // index would be built against.
    migrations::migrate(conn)?;
    conn.execute_batch(INDEXES)?;
    functions::register(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::design::{DesignMaterializationMode, DesignMaterializationPlan};
    use crate::model::{ClauseOrigin, TaskStatus, Verdict};
    use crate::store::{ConformanceAudit, LodestarStore};

    /// Bug: indexes lived in `schema.sql` and therefore ran *before* migrations.
    /// On an existing database `CREATE TABLE IF NOT EXISTS` is a no-op, so the
    /// pre-migration table shape was still in place when
    /// `idx_task_qa_audience` tried to index `task_qa(audience, kind)`. The
    /// batch failed with "no such column: audience", the migration that adds
    /// the column never ran, and **every pre-existing database became
    /// unopenable** by a current build. Found driving a freshly built server
    /// against the live repository database.
    #[test]
    fn opens_a_database_whose_table_predates_a_migrated_column() {
        let path = temporary_database("pre-migration-column");
        {
            // A database as it existed before `audience` was added: the table is
            // real, the column is not.
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE task_qa (
                         id         INTEGER PRIMARY KEY AUTOINCREMENT,
                         task_id    TEXT NOT NULL,
                         kind       TEXT NOT NULL,
                         body       TEXT NOT NULL,
                         author     TEXT NOT NULL,
                         created_at INTEGER NOT NULL
                     );",
                )
                .unwrap();
        }

        let connection = open(path.to_str().unwrap()).expect("a legacy database still opens");

        // The migration ran, so the column exists and the index over it is real.
        let rows: i64 = connection
            .query_row("SELECT count(audience) FROM task_qa", [], |row| row.get(0))
            .expect("the audience column exists after migration");
        assert_eq!(rows, 0);
        let indexes: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master
                 WHERE type = 'index' AND name = 'idx_task_qa_audience'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(indexes, 1, "the index is created after the migration");
    }

    /// Bug: the agent id was `session:v1:{name}:{fingerprint}`, where the name
    /// came from the *hosting process* (`LODESTAR_AGENT`) and the fingerprint
    /// from the session token. One agent whose server started twice with
    /// different environments therefore held two identities. Every comparison
    /// is whole-string equality, so the fleet counted one agent as two, its
    /// claims were split across both halves, and a question addressed to one
    /// half was invisible to the other — which is the bug that made
    /// agent-to-agent dialogue (ADR-0046) undeliverable in practice.
    ///
    /// Observed live on 2026-07-27: `fleet_view` listed
    /// `session:v1:agent:bff9…` holding one claim and
    /// `session:v1:copilot:bff9…` holding two. Same session, same fingerprint,
    /// two rows.
    ///
    /// The fix drops the name from the id, so the migration heals the split:
    /// both halves share the fingerprint and collapse onto one identity that
    /// holds all three claims.
    #[test]
    fn migration_merges_identities_that_forked_on_the_host_process_name() {
        let path = temporary_database("forked-identity");
        let fingerprint = "bff9bbe3968f16636cbc5522086114e3";
        {
            let connection = open(path.to_str().unwrap()).unwrap();
            connection
                .execute_batch(
                    "INSERT INTO goals (id, slug, kind, title, statement, status, created_at)
                     VALUES ('goal:g', 'ship', 'objective', 'Ship', 'Ship it', 'active', 1);",
                )
                .unwrap();
            for (task, half) in [
                ("task:a", "session:v1:agent:"),
                ("task:b", "session:v1:copilot:"),
                ("task:c", "session:v1:copilot:"),
            ] {
                connection
                    .execute(
                        "INSERT INTO tasks (id, goal_id, title, acceptance, status, owner, created_at, updated_at)
                         VALUES (?1, 'goal:g', 'T', 'A', 'claimed', ?2, 1, 1)",
                        rusqlite::params![task, format!("{half}{fingerprint}")],
                    )
                    .unwrap();
            }
            // A question one half addressed to the other: undeliverable before
            // the fix, because neither side answers to the other's id.
            connection
                .execute(
                    "INSERT INTO task_qa (task_id, kind, body, author, audience, created_at)
                     VALUES ('task:a', 'question', 'is your migration landing first?', ?1, ?2, 1)",
                    rusqlite::params![
                        format!("session:v1:agent:{fingerprint}"),
                        format!("session:v1:copilot:{fingerprint}")
                    ],
                )
                .unwrap();
            // Creating the file through `open` records the collapse as applied
            // before any legacy row exists. Clear the marker so this is a
            // database that has genuinely never had the migration run.
            connection
                .execute(
                    "DELETE FROM schema_migrations WHERE name = 'collapse_session_identities'",
                    [],
                )
                .unwrap();
        }

        // Re-opening runs the migration.
        let connection = open(path.to_str().unwrap()).unwrap();

        let collapsed = format!("session:v1:{fingerprint}");
        let owned: i64 = connection
            .query_row(
                "SELECT count(*) FROM tasks WHERE owner = ?1",
                rusqlite::params![collapsed],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owned, 3, "the split agent's claims merge onto one identity");

        let forked: i64 = connection
            .query_row(
                "SELECT count(*) FROM tasks WHERE owner GLOB 'session:v1:*:*'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(forked, 0, "no labelled identity survives");

        // The question now names the same agent that answers to it.
        let (author, audience): (String, String) = connection
            .query_row(
                "SELECT author, audience FROM task_qa WHERE kind = 'question'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(author, collapsed);
        assert_eq!(audience, collapsed);
    }

    /// Bug: the ADR-0054 collapse rewrote `tasks.owner` for every labelled row,
    /// live claims included, and re-fired on every database open because its
    /// idempotence was by *pattern* ("rewrite whatever still looks unmigrated")
    /// rather than by record. A pre-ADR-0054 binary still running against the
    /// same `spec.db` kept minting labelled ids, so each open by a newer binary
    /// re-owned whatever the older one had just claimed.
    ///
    /// Observed live on 2026-07-28 (task:f6daad456855): one session, one token,
    /// `open_session` returning `session:v1:copilot:b4baf280…` while the board
    /// reported the owner as `session:v1:b4baf280…`, flipping between two
    /// consecutive reads with no claim in between. Impact: the holder could not
    /// prove its work (`check_conformance` → "evidence agent does not own the
    /// task"), could not park the task to explain (the owner guard rejected it),
    /// and read as a different owner on re-claim, which opened a fresh evidence
    /// window and orphaned the commit it had already made. A task provable by
    /// nobody is the one outcome the ledger exists to prevent.
    ///
    /// Fix, both halves: ownership of a *live* claim is never rewritten, and the
    /// collapse is recorded in `schema_migrations` so it cannot fire twice.
    #[test]
    fn the_identity_collapse_never_re_owns_a_live_claim_and_cannot_fire_twice() {
        let path = temporary_database("live-claim-ownership");
        let fingerprint = "b4baf2807ebecbd3f43821b330426544";
        let labelled = format!("session:v1:copilot:{fingerprint}");
        let collapsed = format!("session:v1:{fingerprint}");
        let live_until = crate::now_unix() + 3_600;
        {
            let connection = open(path.to_str().unwrap()).unwrap();
            connection
                .execute_batch(
                    "INSERT INTO goals (id, slug, kind, title, statement, status, created_at)
                     VALUES ('goal:g', 'ship', 'objective', 'Ship', 'Ship it', 'active', 1);",
                )
                .unwrap();
            // One claim still held, one whose lease has long since lapsed.
            for (task, lease) in [
                ("task:live", Some(live_until)),
                ("task:lapsed", Some(1_i64)),
            ] {
                connection
                    .execute(
                        "INSERT INTO tasks (id, goal_id, title, acceptance, status, owner, lease_expires_at, created_at, updated_at)
                         VALUES (?1, 'goal:g', 'T', 'A', 'claimed', ?2, ?3, 1, 1)",
                        rusqlite::params![task, labelled, lease],
                    )
                    .unwrap();
            }
            // As above: this database predates the collapse, so it carries no
            // record of it having run.
            connection
                .execute(
                    "DELETE FROM schema_migrations WHERE name = 'collapse_session_identities'",
                    [],
                )
                .unwrap();
        }

        // Re-opening runs the migration.
        let connection = open(path.to_str().unwrap()).unwrap();
        let owner = |connection: &Connection, id: &str| -> String {
            connection
                .query_row(
                    "SELECT owner FROM tasks WHERE id = ?1",
                    rusqlite::params![id],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(
            owner(&connection, "task:live"),
            labelled,
            "a live claim keeps the owner its holder answers to"
        );
        assert_eq!(
            owner(&connection, "task:lapsed"),
            collapsed,
            "a claim nobody is holding is safe to tidy"
        );
        drop(connection);

        // The legacy writer takes a fresh claim under its labelled id, exactly
        // as it did while this was happening.
        {
            let connection = open(path.to_str().unwrap()).unwrap();
            connection
                .execute(
                    "UPDATE tasks SET owner = ?1, lease_expires_at = ?2 WHERE id = 'task:lapsed'",
                    rusqlite::params![labelled, live_until],
                )
                .unwrap();
        }

        // ...and a later open does not reach in and take it away again. Before
        // the fix this rewrite re-fired on every single open, forever.
        let connection = open(path.to_str().unwrap()).unwrap();
        assert_eq!(
            owner(&connection, "task:lapsed"),
            labelled,
            "the collapse is recorded as done and cannot fire a second time"
        );

        let applied: i64 = connection
            .query_row(
                "SELECT count(*) FROM schema_migrations WHERE name = 'collapse_session_identities'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(applied, 1, "recorded once, by name");
    }

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
    fn migration_normalizes_dangling_legacy_predecessors_without_reactivating_work() {
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
            let path = temporary_database(&format!("legacy-dangling-{iteration}"));
            create_legacy_database(&path, false);
            let connection = Connection::open(&path).unwrap();
            connection
                .execute(
                    "UPDATE tasks
                     SET status = ?1, blocked_by = 'task:missing',
                         owner = 'legacy-agent', claim_started_at = 1,
                         lease_expires_at = 999
                     WHERE id = 'task:second'",
                    [legacy],
                )
                .unwrap();
            drop(connection);

            let store = LodestarStore::new(open(path.to_str().unwrap()).unwrap());
            let successor = store.get_task("task:second").unwrap().unwrap();
            assert_eq!(successor.status, expected, "legacy status {legacy}");
            assert!(successor.blocked_by.is_none());
            assert!(successor.owner.is_none());
            assert!(successor.claim_started_at.is_none());
            assert!(successor.lease_expires_at.is_none());
            drop(store);
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn migration_clears_repeated_dangling_predecessors_before_fan_out_validation() {
        let path = temporary_database("legacy-repeated-dangling");
        create_legacy_database(&path, true);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "UPDATE tasks
                 SET status = 'open', blocked_by = 'task:missing'
                 WHERE id IN ('task:second', 'task:third')",
                [],
            )
            .unwrap();
        drop(connection);

        let store = LodestarStore::new(open(path.to_str().unwrap()).unwrap());
        for id in ["task:second", "task:third"] {
            let task = store.get_task(id).unwrap().unwrap();
            assert_eq!(task.status, TaskStatus::Blocked);
            assert!(task.blocked_by.is_none());
        }
        drop(store);
        let connection = Connection::open(&path).unwrap();
        let lineage: i64 = connection
            .query_row("SELECT COUNT(1) FROM task_handoffs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(lineage, 0);
        drop(connection);
        std::fs::remove_file(path).unwrap();
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

    #[test]
    fn migration_records_legacy_materialization_once_and_preserves_repairs() {
        let path = temporary_database("legacy-materialization");
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(SCHEMA).unwrap();
        connection
            .execute_batch(
                "ALTER TABLE design_items ADD COLUMN spawned_goal_id TEXT;
                 INSERT INTO goals
                     (id, slug, kind, title, statement, status, version, created_at)
                 VALUES ('goal:design', 'design', 'objective', 'Design', 'Design', 'active', 1, 1);
                 INSERT INTO design_items
                     (id, adr_path, title, summary, status, proposed_by, decided_by,
                      created_at, updated_at, promotion_status, materialization_revision,
                      spawned_goal_id)
                 VALUES ('design:legacy', 'docs/adr/legacy.md', 'Legacy', '', 'accepted',
                         'planner', 'reviewer', 1, 2, 'materialized', 0, 'goal:design');",
            )
            .unwrap();
        drop(connection);

        let store = LodestarStore::new(open(path.to_str().unwrap()).unwrap());
        let migrated = store.get_design_item("design:legacy").unwrap().unwrap();
        assert_eq!(migrated.materialization_revision, 1);
        assert_eq!(
            store
                .design_materialization_history("design:legacy")
                .unwrap()
                .len(),
            1
        );
        store
            .materialize_design_item(
                "design:legacy",
                &DesignMaterializationPlan {
                    mode: DesignMaterializationMode::NoWork,
                    tasks: Vec::new(),
                    task_ids: Vec::new(),
                    constraints: Vec::new(),
                    rationale: Some("legacy work was already complete".into()),
                },
                "second-reviewer",
                true,
                3,
            )
            .unwrap();
        drop(store);

        let reopened = LodestarStore::new(open(path.to_str().unwrap()).unwrap());
        assert_eq!(
            reopened
                .get_design_item("design:legacy")
                .unwrap()
                .unwrap()
                .materialization_revision,
            2
        );
        assert_eq!(
            reopened
                .design_materialization_history("design:legacy")
                .unwrap()
                .len(),
            2
        );
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn migration_freezes_existing_goals_as_first_local_constitution_version() {
        let path = temporary_database("legacy-constitution");
        create_legacy_database(&path, false);

        let store = LodestarStore::new(open(path.to_str().unwrap()).unwrap());
        let active = store
            .active_constitution_version()
            .unwrap()
            .expect("existing goals freeze into a first active version");
        assert_eq!(active.id, "constitution:v1");
        assert_eq!(active.version, 1);
        // Honest provenance only: migration attributes itself and invents no
        // purpose, preamble, or project identity (SPEC-CONSTITUTION §10).
        assert_eq!(active.created_by.as_deref(), Some("migration"));
        assert!(active.purpose.is_none());
        assert!(active.preamble.is_none());
        assert!(active.project_identity.is_none());

        // The existing clause binds to v1, is locally-authored, and stays
        // review-only because migration invents no enforcement contract.
        let clause = store.get_goal("goal:test").unwrap().unwrap();
        assert_eq!(
            clause.constitution_version.as_deref(),
            Some("constitution:v1")
        );
        assert_eq!(clause.origin, ClauseOrigin::Local);
        assert!(clause.scope.is_none());
        assert!(clause.evidence_contract.is_none());
        assert!(clause.consequence.is_none());
        assert!(!clause.is_enforceable());
        drop(store);

        // Idempotent: a second open adds no version and rebinds no clause.
        let store = LodestarStore::new(open(path.to_str().unwrap()).unwrap());
        drop(store);
        let connection = Connection::open(&path).unwrap();
        let versions: i64 = connection
            .query_row("SELECT COUNT(1) FROM constitution_versions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(versions, 1);
        drop(connection);
        std::fs::remove_file(path).unwrap();
    }

    // ADR-0063: a migration may tidy the past, never the present.
    //
    // `reconnect_superseded_clauses` repairs clauses stranded by an amendment
    // that never recorded its successor, and moving a task's goal is not a
    // cosmetic repair: the goal is what conformance judges the holder's
    // evidence against. Changing it under a live claim would change the rule
    // beneath someone mid-flight, which is the same class of harm as rewriting
    // `tasks.owner` — and not ours to do as a side effect of opening a file.
    //
    // Measured on this repository before the guard: 56 non-terminal tasks would
    // have moved, 3 of them held by an agent with an unexpired lease.
    #[test]
    fn migration_reconnects_stranded_clauses_but_never_a_live_claim() {
        let path = temporary_database("stranded-clauses");
        create_legacy_database(&path, false);

        let now = crate::now_unix();
        let connection = Connection::open(&path).unwrap();
        // A superseded clause with no successor recorded, and its active twin.
        connection
            .execute(
                "UPDATE goals SET status = 'superseded', superseded_by = NULL WHERE id = 'goal:test'",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO goals (id, slug, kind, title, statement, status, version, created_at)
                 VALUES ('goal:test@constitution:v2', 'test', 'objective', 'Test', 'x', 'active', 1, 1)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO goal_code (goal_id, node_id, mode)
                 VALUES ('goal:test', 'artifact:src/a.rs', 'governed')",
                [],
            )
            .unwrap();
        for (id, status, lease) in [
            ("task:idle", "open", None),
            ("task:lapsed", "claimed", Some(now - 60)),
            ("task:live", "claimed", Some(now + 3_600)),
            ("task:finished", "done", None),
        ] {
            connection
                .execute(
                    "INSERT INTO tasks (id, goal_id, title, acceptance, status, lease_expires_at, created_at, updated_at)
                     VALUES (?1, 'goal:test', 't', 'a', ?2, ?3, 1, 1)",
                    rusqlite::params![id, status, lease],
                )
                .unwrap();
        }
        drop(connection);

        let store = LodestarStore::new(open(path.to_str().unwrap()).unwrap());
        drop(store);

        let connection = Connection::open(&path).unwrap();
        let goal_of = |id: &str| -> String {
            connection
                .query_row("SELECT goal_id FROM tasks WHERE id = ?1", [id], |row| {
                    row.get(0)
                })
                .unwrap()
        };
        let successor: Option<String> = connection
            .query_row(
                "SELECT superseded_by FROM goals WHERE id = 'goal:test'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            successor.as_deref(),
            Some("goal:test@constitution:v2"),
            "the stranded clause must name the clause that replaced it"
        );
        assert_eq!(
            goal_of("task:idle"),
            "goal:test@constitution:v2",
            "unclaimed work moves onto the active clause"
        );
        assert_eq!(
            goal_of("task:lapsed"),
            "goal:test@constitution:v2",
            "a claim nobody is holding is safe to tidy"
        );
        assert_eq!(
            goal_of("task:live"),
            "goal:test",
            "a claim someone is holding is not ours to touch (ADR-0063)"
        );
        assert_eq!(
            goal_of("task:finished"),
            "goal:test",
            "finished work keeps naming the clause it was judged under (ADR-0025)"
        );
        let bound: String = connection
            .query_row(
                "SELECT goal_id FROM goal_code WHERE node_id = 'artifact:src/a.rs'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(bound, "goal:test@constitution:v2");
        drop(connection);
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

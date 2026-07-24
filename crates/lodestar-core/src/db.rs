//! SQLite connection setup, schema application, and the `effective_weight`
//! scalar function used for knowledge revalidation queries.

mod functions;
mod migrations;

use rusqlite::Connection;

use crate::error::Result;

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
    migrations::migrate(conn)?;
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

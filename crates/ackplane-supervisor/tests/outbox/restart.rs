use super::*;
use ackplane_supervisor::{InboxError, SupervisorInbox};

// Regenerating the start time under one session ID made restart conflict with
// the server's immutable session. Restore the original declaration from disk.
#[test]
fn reopening_an_outbox_preserves_the_original_immutable_session_start() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.db");
    let original = session();
    let outbox = SupervisorOutbox::open(&path, registration(), original.clone()).unwrap();
    outbox.enqueue(1, &heartbeat(1)).unwrap();
    drop(outbox);

    let mut restarted = original.clone();
    restarted.started_at += 100;
    let reopened = SupervisorOutbox::open(&path, registration(), restarted.clone()).unwrap();
    assert_eq!(reopened.session(), &original);
    let inspected = SupervisorOutbox::open_read_only(&path, registration(), restarted).unwrap();
    assert_eq!(inspected.session(), &original);
    assert_eq!(
        inspected.pending(10).unwrap(),
        reopened.pending(10).unwrap()
    );
}

#[test]
fn reopening_refuses_a_changed_worker_runtime_or_initial_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.db");
    drop(SupervisorOutbox::open(&path, registration(), session()).unwrap());
    let mut worker = session();
    worker.worker_id = "another-worker".into();
    let mut runtime = session();
    runtime.runtime = SupervisorRuntime::LocalMachine;
    let mut state = session();
    state.state = SupervisorWorkerState::Completed;

    for changed in [worker, runtime, state] {
        assert!(matches!(
            SupervisorOutbox::open(&path, registration(), changed),
            Err(OutboxError::OutboxIdentityMismatch)
        ));
        let reopened = SupervisorOutbox::open_read_only(&path, registration(), session()).unwrap();
        assert_eq!(reopened.session(), &session());
    }
}

#[test]
fn inbox_requires_the_same_original_session_as_the_outbox() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("inbox.db");
    drop(SupervisorInbox::open(&path, registration(), session()).unwrap());
    let mut changed = session();
    changed.started_at += 100;
    assert!(matches!(
        SupervisorInbox::open(&path, registration(), changed),
        Err(InboxError::InboxIdentityMismatch)
    ));
    drop(SupervisorInbox::open(&path, registration(), session()).unwrap());
    rusqlite::Connection::open(&path)
        .unwrap()
        .execute("DELETE FROM supervisor_session", [])
        .unwrap();
    assert!(matches!(
        SupervisorInbox::open(&path, registration(), session()),
        Err(InboxError::MissingSession)
    ));
}

#[test]
fn missing_session_history_is_refused_without_replacing_it_or_pending_evidence() {
    for remove_table in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("outbox.db");
        let frame = heartbeat(1);
        let outbox = SupervisorOutbox::open(&path, registration(), session()).unwrap();
        outbox.enqueue(1, &frame).unwrap();
        drop(outbox);
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(if remove_table {
                "DROP TABLE supervisor_session"
            } else {
                "DELETE FROM supervisor_session"
            })
            .unwrap();
        assert!(matches!(
            SupervisorOutbox::open_read_only(&path, registration(), session()),
            Err(OutboxError::MissingSession)
        ));
        let table_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'supervisor_session')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(table_exists, !remove_table);
        assert!(matches!(
            SupervisorOutbox::open(&path, registration(), session()),
            Err(OutboxError::MissingSession)
        ));
        let count: u64 = connection
            .query_row("SELECT COUNT(*) FROM supervisor_session", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 0, "refusal must not invent an original session");
        let retained: Vec<u8> = connection
            .query_row(
                "SELECT frame FROM outbound_frames WHERE sequence = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(retained, frame.encode_to_vec());
    }
}

#[test]
fn failed_session_persistence_cannot_leave_a_partially_bound_queue() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("outbox.db");
    drop(SupervisorOutbox::open(&path, registration(), session()).unwrap());
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "DELETE FROM supervisor_session; DELETE FROM inbox_identity;
             CREATE TRIGGER reject_session BEFORE INSERT ON supervisor_session
             BEGIN SELECT RAISE(ABORT, 'injected session write failure'); END;",
        )
        .unwrap();
    assert!(matches!(
        SupervisorOutbox::open(&path, registration(), session()),
        Err(OutboxError::Database(_))
    ));
    let count: u64 = connection
        .query_row("SELECT COUNT(*) FROM inbox_identity", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0, "identity and session must commit atomically");
    connection
        .execute_batch("DROP TRIGGER reject_session")
        .unwrap();
    let reopened = SupervisorOutbox::open(&path, registration(), session()).unwrap();
    assert_eq!(reopened.session(), &session());
}

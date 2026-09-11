use super::{heartbeat, lifecycle, registration, session};
use ackplane_protocol::v1;
use ackplane_supervisor::{OutboxError, OutboxPositions, SupervisorOutbox};

// Inspection reused the writable initializer. Open existing evidence without
// migrations, and let SQLite refuse mutations instead of trusting the caller.
#[test]
fn read_only_outbox_reads_live_evidence_and_refuses_every_mutation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("outbox.db");
    let writer = SupervisorOutbox::open(&path, registration(), session()).unwrap();
    writer
        .enqueue_next(lifecycle(v1::SupervisorWorkerState::Terminated, None))
        .unwrap();
    writer.acknowledge_through(1).unwrap();
    writer.enqueue(2, &heartbeat(10)).unwrap();
    let expected_pending = writer.pending(10).unwrap();
    let expected_history = writer.acknowledged_lifecycle_receipts(0, 10).unwrap();

    let reader = SupervisorOutbox::open_read_only(&path, registration(), session()).unwrap();
    assert_eq!(reader.pending(10).unwrap(), expected_pending);
    assert_eq!(
        reader.acknowledged_lifecycle_receipts(0, 10).unwrap(),
        expected_history
    );
    assert_eq!(reader.identity(), &registration().identity);
    assert_eq!(reader.session(), &session());
    for result in [
        reader.enqueue(3, &heartbeat(20)).map(drop),
        reader
            .enqueue_next(lifecycle(v1::SupervisorWorkerState::Completed, None))
            .map(drop),
        reader.acknowledge_through(2).map(drop),
    ] {
        let OutboxError::Database(error) = result.unwrap_err() else {
            panic!("SQLite must refuse mutation on the read-only connection");
        };
        assert_eq!(
            error.sqlite_error_code(),
            Some(rusqlite::ErrorCode::ReadOnly)
        );
    }
    assert_eq!(writer.pending(10).unwrap(), expected_pending);
    assert_eq!(
        writer.acknowledged_lifecycle_receipts(0, 10).unwrap(),
        expected_history
    );
    assert_eq!(
        reader.positions().unwrap(),
        OutboxPositions {
            acknowledged: 1,
            last_enqueued: 2
        }
    );

    writer.enqueue(3, &heartbeat(30)).unwrap();
    assert_eq!(reader.pending(10).unwrap(), writer.pending(10).unwrap());
    assert_eq!(
        reader.positions().unwrap(),
        OutboxPositions {
            acknowledged: 1,
            last_enqueued: 3
        }
    );
}

#[test]
fn read_only_open_does_not_create_or_repair_missing_empty_or_corrupt_databases() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing/outbox.db");
    assert!(SupervisorOutbox::open_read_only(&missing, registration(), session()).is_err());
    assert!(!missing.parent().unwrap().exists());

    let path = root.path().join("outbox.db");
    for contents in [b"".as_slice(), b"not a SQLite database".as_slice()] {
        std::fs::write(&path, contents).unwrap();
        assert!(SupervisorOutbox::open_read_only(&path, registration(), session()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), contents);
    }
}

#[test]
fn read_only_open_rejects_sqlite_special_writable_targets() {
    for path in [":memory:", ""] {
        assert!(
            SupervisorOutbox::open_read_only(path, registration(), session()).is_err(),
            "inspection must not become a writable database for {path:?}"
        );
    }
}

#[test]
fn read_only_open_does_not_upgrade_older_schema_or_change_journal_mode() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("outbox.db");
    let writer = SupervisorOutbox::open(&path, registration(), session()).unwrap();
    writer.enqueue(1, &heartbeat(10)).unwrap();
    let expected = writer.pending(10).unwrap();
    drop(writer);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch("DROP TABLE acknowledged_lifecycle_receipts")
        .unwrap();
    connection
        .pragma_update(None, "journal_mode", "DELETE")
        .unwrap();
    let schema_version: i64 = connection
        .pragma_query_value(None, "schema_version", |row| row.get(0))
        .unwrap();
    drop(connection);

    let reader = SupervisorOutbox::open_read_only(&path, registration(), session()).unwrap();
    assert_eq!(reader.pending(10).unwrap(), expected);
    assert_eq!(
        reader.positions().unwrap(),
        OutboxPositions {
            acknowledged: 0,
            last_enqueued: 1
        }
    );
    assert!(matches!(
        reader.acknowledged_lifecycle_receipts(0, 10),
        Err(OutboxError::Database(_))
    ));
    drop(reader);

    let connection = rusqlite::Connection::open(&path).unwrap();
    let after_version: i64 = connection
        .pragma_query_value(None, "schema_version", |row| row.get(0))
        .unwrap();
    let journal: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    let archive_tables: i64 = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'acknowledged_lifecycle_receipts'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(after_version, schema_version);
    assert_eq!(journal, "delete");
    assert_eq!(archive_tables, 0);
}

#[test]
fn read_only_open_refuses_a_missing_identity_without_adopting_the_database() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("outbox.db");
    drop(SupervisorOutbox::open(&path, registration(), session()).unwrap());
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute("DELETE FROM inbox_identity", [])
        .unwrap();

    assert!(matches!(
        SupervisorOutbox::open_read_only(&path, registration(), session()),
        Err(OutboxError::OutboxIdentityMismatch)
    ));
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM inbox_identity", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn read_only_open_preserves_identity_and_session_isolation() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("outbox.db");
    let writer = SupervisorOutbox::open(&path, registration(), session()).unwrap();
    writer
        .enqueue_next(lifecycle(v1::SupervisorWorkerState::Completed, None))
        .unwrap();
    writer.acknowledge_through(1).unwrap();
    let expected = writer.acknowledged_lifecycle_receipts(0, 10).unwrap();
    drop(writer);

    let mut other_registration = registration();
    other_registration.identity.tenant_id = "another-tenant".into();
    let mut other_session = session();
    other_session.session_id = "another-session".into();
    for (declared_registration, declared_session) in [
        (other_registration, session()),
        (registration(), other_session),
    ] {
        assert!(matches!(
            SupervisorOutbox::open_read_only(&path, declared_registration, declared_session),
            Err(OutboxError::OutboxIdentityMismatch)
        ));
    }
    let reader = SupervisorOutbox::open_read_only(&path, registration(), session()).unwrap();
    assert_eq!(
        reader.acknowledged_lifecycle_receipts(0, 10).unwrap(),
        expected
    );
    assert_eq!(
        reader.positions().unwrap(),
        OutboxPositions {
            acknowledged: 1,
            last_enqueued: 1
        }
    );
}

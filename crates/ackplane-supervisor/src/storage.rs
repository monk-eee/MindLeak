//! SQLite schema and persistence helpers for durable supervisor queues.

use std::{io, path::Path, time::Duration};

use ackplane_protocol::{
    supervisor::{SupervisorIdentity, SupervisorSession},
    v1,
};
use prost::Message;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS inbox_identity (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    tenant_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    supervisor_id TEXT NOT NULL,
    session_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS directive_inbox (
    directive_id TEXT PRIMARY KEY,
    payload_digest BLOB NOT NULL,
    tenant_id TEXT NOT NULL,
    project_id TEXT NOT NULL,
    repository_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    sequence INTEGER NOT NULL UNIQUE,
    receipt_status INTEGER NOT NULL,
    receipt_reason INTEGER NOT NULL,
    occurred_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS outbound_frames (
    sequence INTEGER PRIMARY KEY,
    frame BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS acknowledged_lifecycle_receipts (
    sequence INTEGER PRIMARY KEY,
    frame BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS stopped_worker_run (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    marker BLOB NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence > 0),
    frame BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS worker_recovery_events (
    confirmation_digest TEXT NOT NULL,
    event TEXT NOT NULL CHECK (event IN ('started', 'completed')),
    marker_digest TEXT NOT NULL,
    reason TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    result_digest TEXT,
    server_position INTEGER,
    lease_released INTEGER,
    PRIMARY KEY (confirmation_digest, event),
    CHECK ((event = 'started' AND result_digest IS NULL AND server_position IS NULL AND lease_released IS NULL)
        OR (event = 'completed' AND result_digest IS NOT NULL AND server_position >= 0 AND lease_released IN (0, 1)))
);

CREATE TABLE IF NOT EXISTS directive_effects (
    directive_id TEXT PRIMARY KEY REFERENCES directive_inbox(directive_id),
    receipt BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS outbound_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    last_sequence INTEGER NOT NULL CHECK (last_sequence >= 0)
);

INSERT OR IGNORE INTO outbound_state (singleton, last_sequence)
SELECT 1, COALESCE(MAX(sequence), 0) FROM outbound_frames;
"#;

pub(crate) fn configure(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.execute_batch(SCHEMA)
}

/// Retain the separate lock connection for the daemon lifetime; never unlink its file.
pub(crate) fn claim_state_directory(directory: &Path) -> io::Result<Connection> {
    std::fs::create_dir_all(directory)?;
    let connection = Connection::open(directory.join("ownership.db")).map_err(io::Error::other)?;
    connection
        .busy_timeout(Duration::ZERO)
        .map_err(io::Error::other)?;
    match connection.execute_batch("BEGIN EXCLUSIVE") {
        Ok(()) => Ok(connection),
        Err(error) if matches!(error.sqlite_error_code(), Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked)) => {
            Err(io::Error::new(io::ErrorKind::WouldBlock, format!(
                "state directory is already in use: {}; stop its current supervisor or configure a separate state directory",
                directory.display(),
            )))
        }
        Err(error) => Err(io::Error::other(error)),
    }
}

pub(crate) struct StoredReceipt {
    pub(crate) payload_digest: Vec<u8>,
    directive_id: String,
    tenant_id: String,
    project_id: String,
    repository_id: String,
    node_id: String,
    agent_session_id: String,
    sequence: u64,
    status: i32,
    reason: i32,
    occurred_at: String,
}

impl StoredReceipt {
    pub(crate) fn into_receipt(self) -> v1::DirectiveReceipt {
        v1::DirectiveReceipt {
            directive_id: self.directive_id,
            tenant_id: self.tenant_id,
            project_id: self.project_id,
            repository_id: self.repository_id,
            node_id: self.node_id,
            agent_session_id: self.agent_session_id,
            status: self.status,
            reason: self.reason,
            occurred_at: self.occurred_at,
            payload_digest: self.payload_digest,
            checkpoint_refs: Vec::new(),
            evidence_refs: Vec::new(),
            directive_sequence: self.sequence,
            diagnostic: String::new(),
            // The inbox records what this supervisor decided; the outbox
            // decides where that decision sits in the resendable stream. A
            // receipt rebuilt from inbox state has not been enqueued yet, so
            // it carries no position until `enqueue_receipt` stamps one
            // (ADR-0146 decision 1).
            outbox_sequence: None,
        }
    }
}

pub(crate) fn ensure_supervisor_identity(
    conn: &Connection,
    identity: &SupervisorIdentity,
    supervisor_id: &str,
    session: &SupervisorSession,
) -> Result<bool, rusqlite::Error> {
    let stored: Option<(String, String, String, String, String)> = conn
        .query_row(
            "SELECT tenant_id, repository_id, node_id, supervisor_id, session_id FROM inbox_identity WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .optional()?;
    if let Some(stored) = stored {
        if stored
            != (
                identity.tenant_id.clone(),
                identity.repository_id.clone(),
                identity.node_id.clone(),
                supervisor_id.to_string(),
                session.session_id.clone(),
            )
        {
            return Ok(false);
        }
        return Ok(true);
    }
    if conn.is_readonly(rusqlite::DatabaseName::Main)? {
        return Ok(false);
    }
    conn.execute(
        "INSERT INTO inbox_identity (singleton, tenant_id, repository_id, node_id, supervisor_id, session_id) VALUES (1, ?1, ?2, ?3, ?4, ?5)",
        params![
            identity.tenant_id,
            identity.repository_id,
            identity.node_id,
            supervisor_id,
            session.session_id,
        ],
    )?;
    Ok(true)
}

pub(crate) fn next_sequence(transaction: &Transaction<'_>) -> Result<i64, rusqlite::Error> {
    transaction.query_row(
        "SELECT COALESCE(MAX(sequence), 0) + 1 FROM directive_inbox",
        [],
        |row| row.get(0),
    )
}

pub(crate) fn load_effect(
    conn: &Connection,
    directive_id: &str,
) -> Result<Option<Vec<u8>>, rusqlite::Error> {
    conn.query_row(
        "SELECT receipt FROM directive_effects WHERE directive_id = ?1",
        [directive_id],
        |row| row.get(0),
    )
    .optional()
}

pub(crate) fn store_effect(
    conn: &Connection,
    directive_id: &str,
    receipt: &[u8],
) -> Result<(), rusqlite::Error> {
    conn.execute("INSERT INTO directive_effects (directive_id, receipt) VALUES (?1, ?2) ON CONFLICT (directive_id) DO UPDATE SET receipt = excluded.receipt", params![directive_id, receipt])?;
    Ok(())
}

pub(crate) fn load_receipt(
    transaction: &Transaction<'_>,
    directive_id: &str,
) -> Result<Option<StoredReceipt>, rusqlite::Error> {
    transaction
        .query_row(
            "SELECT payload_digest, directive_id, tenant_id, project_id, repository_id, node_id, session_id, sequence, receipt_status, receipt_reason, occurred_at FROM directive_inbox WHERE directive_id = ?1",
            params![directive_id],
            |row| {
                Ok(StoredReceipt {
                    payload_digest: row.get(0)?,
                    directive_id: row.get(1)?,
                    tenant_id: row.get(2)?,
                    project_id: row.get(3)?,
                    repository_id: row.get(4)?,
                    node_id: row.get(5)?,
                    agent_session_id: row.get(6)?,
                    sequence: row.get(7)?,
                    status: row.get(8)?,
                    reason: row.get(9)?,
                    occurred_at: row.get(10)?,
                })
            },
        )
        .optional()
}

pub(crate) fn store_receipt(
    transaction: &Transaction<'_>,
    receipt: &v1::DirectiveReceipt,
) -> Result<(), rusqlite::Error> {
    transaction.execute(
        "INSERT INTO directive_inbox (directive_id, payload_digest, tenant_id, project_id, repository_id, node_id, session_id, sequence, receipt_status, receipt_reason, occurred_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            receipt.directive_id,
            receipt.payload_digest,
            receipt.tenant_id,
            receipt.project_id,
            receipt.repository_id,
            receipt.node_id,
            receipt.agent_session_id,
            receipt.directive_sequence,
            receipt.status,
            receipt.reason,
            receipt.occurred_at,
        ],
    )?;
    Ok(())
}

pub(crate) fn next_outbound_sequence(
    transaction: &Transaction<'_>,
) -> Result<i64, rusqlite::Error> {
    transaction.query_row(
        "SELECT last_sequence + 1 FROM outbound_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )
}

pub(crate) fn load_outbound_frame(
    transaction: &Transaction<'_>,
    sequence: i64,
) -> Result<Option<Vec<u8>>, rusqlite::Error> {
    transaction
        .query_row(
            "SELECT frame FROM outbound_frames WHERE sequence = ?1",
            params![sequence],
            |row| row.get(0),
        )
        .optional()
}

pub(crate) fn store_outbound_frame(
    transaction: &Transaction<'_>,
    sequence: i64,
    frame: &[u8],
) -> Result<(), rusqlite::Error> {
    transaction.execute(
        "INSERT INTO outbound_frames (sequence, frame) VALUES (?1, ?2)",
        params![sequence, frame],
    )?;
    Ok(())
}

pub(crate) fn record_outbound_sequence(
    transaction: &Transaction<'_>,
    sequence: i64,
) -> Result<(), rusqlite::Error> {
    transaction.execute(
        "UPDATE outbound_state SET last_sequence = ?1 WHERE singleton = 1",
        params![sequence],
    )?;
    Ok(())
}

pub(crate) fn pending_outbound_frames(
    conn: &Connection,
    limit: i64,
) -> Result<Vec<(u64, Vec<u8>)>, rusqlite::Error> {
    let mut statement =
        conn.prepare("SELECT sequence, frame FROM outbound_frames ORDER BY sequence ASC LIMIT ?1")?;
    let rows = statement.query_map(params![limit], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

pub(crate) fn acknowledge_outbound_frames(
    transaction: &Transaction<'_>,
    sequence: i64,
) -> Result<usize, rusqlite::Error> {
    {
        let mut statement = transaction.prepare(
            "SELECT sequence, frame FROM outbound_frames WHERE sequence <= ?1 ORDER BY sequence ASC",
        )?;
        let rows = statement.query_map(params![sequence], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        for row in rows {
            let (stored_sequence, bytes) = row?;
            let frame = v1::NodeFrame::decode(bytes.as_slice()).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Blob,
                    Box::new(error),
                )
            })?;
            if matches!(
                frame.frame,
                Some(v1::node_frame::Frame::SupervisorLifecycleReceipt(_))
            ) {
                transaction.execute(
                    "INSERT INTO acknowledged_lifecycle_receipts (sequence, frame) VALUES (?1, ?2)",
                    params![stored_sequence, bytes],
                )?;
            }
        }
    }
    transaction.execute(
        "DELETE FROM outbound_frames WHERE sequence <= ?1",
        params![sequence],
    )
}

pub(crate) fn acknowledged_lifecycle_receipts(
    conn: &Connection,
    after_sequence: i64,
    limit: i64,
) -> Result<Vec<(u64, Vec<u8>)>, rusqlite::Error> {
    let mut statement = conn.prepare(
        "SELECT sequence, frame FROM acknowledged_lifecycle_receipts WHERE sequence > ?1 ORDER BY sequence ASC LIMIT ?2",
    )?;
    let rows = statement.query_map(params![after_sequence, limit], |row| {
        Ok((row.get(0)?, row.get(1)?))
    })?;
    rows.collect()
}

/// The highest sequence this outbox can prove was acknowledged, and the
/// highest it ever enqueued.
///
/// The acknowledged position is derived rather than stored: acknowledgement
/// deletes frames, so the boundary is the sequence below the oldest surviving
/// frame, or the whole queue when none survive. A second stored counter would
/// be a value that can disagree with the rows it summarises, and the disagreement
/// would only ever be discovered during a reconnect -- the one moment the number
/// has to be trusted.
pub(crate) fn outbound_positions(conn: &Connection) -> Result<(i64, i64), rusqlite::Error> {
    let last_enqueued: i64 = conn.query_row(
        "SELECT last_sequence FROM outbound_state WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    let oldest_pending: Option<i64> =
        conn.query_row("SELECT MIN(sequence) FROM outbound_frames", [], |row| {
            row.get(0)
        })?;
    let acknowledged = match oldest_pending {
        Some(oldest) => oldest - 1,
        None => last_enqueued,
    };
    Ok((acknowledged, last_enqueued))
}

#[cfg(test)]
mod tests {
    #[test]
    fn state_ownership_releases_on_drop_without_removing_recovery_evidence() {
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("first.worker-run.json");
        std::fs::write(&marker, "unaccounted worker").unwrap();
        let owner = super::claim_state_directory(directory.path()).unwrap();
        let refused = super::claim_state_directory(&directory.path().join(".")).unwrap_err();
        assert_eq!(refused.kind(), std::io::ErrorKind::WouldBlock);
        drop(owner);

        let _new_owner = super::claim_state_directory(directory.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(marker).unwrap(),
            "unaccounted worker"
        );
        assert!(directory.path().join("ownership.db").exists());
    }
}

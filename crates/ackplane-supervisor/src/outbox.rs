//! Durable outbound NodeFrame queue for one enrolled supervisor session.

use std::{fs, path::Path};

use ackplane_protocol::{
    supervisor::{SupervisorError, SupervisorRegistration, SupervisorSession},
    v1,
};
use prost::Message;
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};
use thiserror::Error;

use crate::storage::{
    acknowledge_outbound_frames, configure, ensure_supervisor_identity, load_outbound_frame,
    next_outbound_sequence, pending_outbound_frames, record_outbound_sequence,
    store_outbound_frame,
};

mod recovery;
pub(crate) use recovery::{RecoveryAttempt, RecoveryResult};

/// A path-owned local outbox for frames awaiting a future NodeSync sender.
pub struct SupervisorOutbox {
    conn: Connection,
    registration: SupervisorRegistration,
    session: SupervisorSession,
}

impl SupervisorOutbox {
    /// Open or create the outbox at `path`, bound to one supervisor identity and session.
    pub fn open(
        path: impl AsRef<Path>,
        registration: SupervisorRegistration,
        session: SupervisorSession,
    ) -> Result<Self, OutboxError> {
        if let Some(parent) = path.as_ref().parent() {
            fs::create_dir_all(parent)?;
        }
        Self::from_connection(Connection::open(path)?, registration, session)
    }

    /// Inspect an existing identity-bound outbox through SQLite read-only access.
    /// No directories, schema or identity records are created; mutation methods fail.
    pub fn open_read_only(
        path: impl AsRef<Path>,
        registration: SupervisorRegistration,
        session: SupervisorSession,
    ) -> Result<Self, OutboxError> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Self::from_connection(conn, registration, session)
    }

    /// Build an ephemeral outbox for focused tests and tooling.
    pub fn open_in_memory(
        registration: SupervisorRegistration,
        session: SupervisorSession,
    ) -> Result<Self, OutboxError> {
        Self::from_connection(Connection::open_in_memory()?, registration, session)
    }

    fn from_connection(
        conn: Connection,
        registration: SupervisorRegistration,
        session: SupervisorSession,
    ) -> Result<Self, OutboxError> {
        registration.validate()?;
        session.validate()?;
        if session.supervisor_id != registration.supervisor_id {
            return Err(OutboxError::SessionSupervisorMismatch);
        }
        if !conn.is_readonly(rusqlite::DatabaseName::Main)? {
            configure(&conn)?;
        }
        if !ensure_supervisor_identity(
            &conn,
            &registration.identity,
            &registration.supervisor_id,
            &session,
        )? {
            return Err(OutboxError::OutboxIdentityMismatch);
        }
        Ok(Self {
            conn,
            registration,
            session,
        })
    }

    /// Persist `frame` before a future sender is allowed to transmit it.
    pub fn enqueue(
        &self,
        sequence: u64,
        frame: &v1::NodeFrame,
    ) -> Result<QueueOutcome, OutboxError> {
        let sequence = positive_sequence(sequence)?;
        let encoded = frame.encode_to_vec();
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;

        if let Some(existing) = load_outbound_frame(&transaction, sequence)? {
            if existing == encoded {
                return Ok(QueueOutcome::Replayed);
            }
            return Err(OutboxError::FrameConflict {
                sequence: sequence as u64,
            });
        }

        let expected = next_outbound_sequence(&transaction)?;
        if sequence != expected {
            return Err(OutboxError::SequenceGap {
                expected: expected as u64,
                received: sequence as u64,
            });
        }

        store_outbound_frame(&transaction, sequence, &encoded)?;
        record_outbound_sequence(&transaction, sequence)?;
        transaction.commit()?;
        Ok(QueueOutcome::Queued)
    }

    /// Allocate and stamp a durable receipt frame in the same transaction.
    pub fn enqueue_next(&self, frame: v1::NodeFrame) -> Result<QueuedFrame, OutboxError> {
        self.enqueue_with_stop(frame, None)
    }

    pub(crate) fn enqueue_with_stop(
        &self,
        mut frame: v1::NodeFrame,
        marker: Option<&[u8]>,
    ) -> Result<QueuedFrame, OutboxError> {
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let sequence = next_outbound_sequence(&transaction)?;
        match frame.frame.as_mut() {
            Some(v1::node_frame::Frame::DirectiveReceipt(receipt)) => {
                receipt.outbox_sequence = Some(sequence as u64)
            }
            Some(v1::node_frame::Frame::SupervisorLifecycleReceipt(receipt)) => {
                receipt.outbox_sequence = Some(sequence as u64)
            }
            Some(v1::node_frame::Frame::ContextPacketUseReport(receipt)) => {
                receipt.outbox_sequence = Some(sequence as u64)
            }
            _ => return Err(OutboxError::UnsupportedFrame),
        }
        let encoded = frame.encode_to_vec();
        if let Some(marker) = marker {
            let Some(v1::node_frame::Frame::SupervisorLifecycleReceipt(receipt)) = &frame.frame
            else {
                return Err(OutboxError::RecoveryEvidence(
                    "stop record requires a lifecycle receipt".into(),
                ));
            };
            if marker.is_empty()
                || marker.len() > 64 * 1024
                || receipt.supervisor_id != self.session.supervisor_id
                || receipt.session_id != self.session.session_id
                || receipt.worker_id != self.session.worker_id
                || !matches!(
                    v1::SupervisorWorkerState::try_from(receipt.state),
                    Ok(v1::SupervisorWorkerState::Terminated
                        | v1::SupervisorWorkerState::Completed
                        | v1::SupervisorWorkerState::Failed)
                )
            {
                return Err(OutboxError::RecoveryEvidence(
                    "stop record does not describe this terminal worker".into(),
                ));
            }
            transaction.execute(
                "INSERT INTO stopped_worker_run (singleton, marker, sequence, frame) VALUES (1, ?1, ?2, ?3)",
                rusqlite::params![marker, sequence, encoded],
            )?;
        }
        store_outbound_frame(&transaction, sequence, &encoded)?;
        record_outbound_sequence(&transaction, sequence)?;
        transaction.commit()?;
        Ok(QueuedFrame {
            sequence: sequence as u64,
            frame,
        })
    }

    /// Return the oldest pending frames in local sequence order.
    /// Refuse encodings this version cannot replay byte-for-byte.
    pub fn pending(&self, limit: u32) -> Result<Vec<QueuedFrame>, OutboxError> {
        if limit == 0 {
            return Err(OutboxError::NonPositiveLimit);
        }
        pending_outbound_frames(&self.conn, i64::from(limit))?
            .into_iter()
            .map(QueuedFrame::decode)
            .collect()
    }

    /// Read acknowledged lifecycle receipts after a sequence, capped at 100 per page.
    ///
    /// These are original local reports, not proof of current process state or task
    /// completion. Receipts pruned before retention was enabled cannot be recovered.
    pub fn acknowledged_lifecycle_receipts(
        &self,
        after_sequence: u64,
        limit: u32,
    ) -> Result<Vec<QueuedFrame>, OutboxError> {
        if limit == 0 {
            return Err(OutboxError::NonPositiveLimit);
        }
        let after_sequence =
            i64::try_from(after_sequence).map_err(|_| OutboxError::SequenceOutOfRange)?;
        crate::storage::acknowledged_lifecycle_receipts(
            &self.conn,
            after_sequence,
            i64::from(limit.min(100)),
        )?
        .into_iter()
        .map(QueuedFrame::decode)
        .collect()
    }

    /// Acknowledge every frame at or below an accepted local sequence position.
    /// Original lifecycle reports are retained atomically; they are not pending delivery.
    pub fn acknowledge_through(&self, sequence: u64) -> Result<usize, OutboxError> {
        let sequence = positive_sequence(sequence)?;
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let removed = acknowledge_outbound_frames(&transaction, sequence)?;
        transaction.commit()?;
        Ok(removed)
    }

    /// The configured identity is retained to prevent another node reopening this database.
    pub fn identity(&self) -> &ackplane_protocol::supervisor::SupervisorIdentity {
        &self.registration.identity
    }

    /// The configured session is retained to prevent cross-session queue reuse.
    pub fn session(&self) -> &SupervisorSession {
        &self.session
    }

    /// What this outbox can prove about its own progress, for reconnect
    /// reconciliation (ADR-0116 decision 7).
    pub fn positions(&self) -> Result<OutboxPositions, OutboxError> {
        let (acknowledged, last_enqueued) = crate::storage::outbound_positions(&self.conn)?;
        Ok(OutboxPositions {
            acknowledged: acknowledged.max(0) as u64,
            last_enqueued: last_enqueued.max(0) as u64,
        })
    }
}

/// One outbox's durable progress: what it can prove was accepted, and what it
/// has queued behind that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutboxPositions {
    /// The highest sequence this supervisor can prove the server accepted.
    pub acknowledged: u64,
    /// The highest sequence this supervisor ever enqueued locally.
    pub last_enqueued: u64,
}

/// Whether an enqueue inserted a frame or replayed an identical durable frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueOutcome {
    Queued,
    Replayed,
}

/// One pending frame and the local sequence a future sender will acknowledge.
#[derive(Debug, Clone, PartialEq)]
pub struct QueuedFrame {
    pub sequence: u64,
    pub frame: v1::NodeFrame,
}

impl QueuedFrame {
    fn decode((sequence, bytes): (u64, Vec<u8>)) -> Result<Self, OutboxError> {
        let frame = v1::NodeFrame::decode(bytes.as_slice())
            .map_err(|_| OutboxError::CorruptStoredFrame { sequence })?;
        if frame.encode_to_vec() != bytes {
            return Err(OutboxError::UnsupportedStoredEncoding { sequence });
        }
        Ok(Self { sequence, frame })
    }
}

/// Durable-outbox errors are explicit so a future transport never guesses delivery state.
#[derive(Debug, Error)]
pub enum OutboxError {
    #[error("frame does not support durable supervisor delivery")]
    UnsupportedFrame,
    #[error("invalid supervisor declaration: {0}")]
    Supervisor(#[from] SupervisorError),
    #[error("supervisor session does not belong to the configured supervisor")]
    SessionSupervisorMismatch,
    #[error("outbox I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("outbox database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("the durable outbox is already bound to another supervisor identity or session")]
    OutboxIdentityMismatch,
    #[error("outbox sequence must be positive")]
    SequenceMustBePositive,
    #[error("outbox sequence exceeds the local range")]
    SequenceOutOfRange,
    #[error("outbox sequence gap: expected {expected}, got {received}")]
    SequenceGap { expected: u64, received: u64 },
    #[error("outbox sequence {sequence} was replayed with different frame bytes")]
    FrameConflict { sequence: u64 },
    #[error("outbox pending limit must be positive")]
    NonPositiveLimit,
    #[error("stored outbox frame at sequence {sequence} cannot be decoded")]
    CorruptStoredFrame { sequence: u64 },
    #[error("stored outbox frame at sequence {sequence} contains wire bytes this version cannot replay identically; evidence retained")]
    UnsupportedStoredEncoding { sequence: u64 },
    #[error("worker recovery evidence is inconsistent: {0}")]
    RecoveryEvidence(String),
}

fn positive_sequence(sequence: u64) -> Result<i64, OutboxError> {
    let sequence = i64::try_from(sequence).map_err(|_| OutboxError::SequenceOutOfRange)?;
    if sequence <= 0 {
        return Err(OutboxError::SequenceMustBePositive);
    }
    Ok(sequence)
}

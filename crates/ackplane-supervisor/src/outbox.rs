//! Durable outbound NodeFrame queue for one enrolled supervisor session.

use std::{fs, path::Path};

use ackplane_protocol::{
    supervisor::{SupervisorError, SupervisorRegistration, SupervisorSession},
    v1,
};
use prost::Message;
use rusqlite::{Connection, Transaction, TransactionBehavior};
use thiserror::Error;

use crate::storage::{
    acknowledge_outbound_frames, configure, ensure_supervisor_identity, load_outbound_frame,
    next_outbound_sequence, pending_outbound_frames, record_outbound_sequence,
    store_outbound_frame,
};

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
        configure(&conn)?;
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

    /// Return the oldest pending frames in local sequence order.
    pub fn pending(&self, limit: u32) -> Result<Vec<QueuedFrame>, OutboxError> {
        if limit == 0 {
            return Err(OutboxError::NonPositiveLimit);
        }
        pending_outbound_frames(&self.conn, i64::from(limit))?
            .into_iter()
            .map(|(sequence, bytes)| {
                let frame = v1::NodeFrame::decode(bytes.as_slice())
                    .map_err(|_| OutboxError::CorruptStoredFrame { sequence })?;
                Ok(QueuedFrame { sequence, frame })
            })
            .collect()
    }

    /// Acknowledge every frame at or below an accepted local sequence position.
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

/// Durable-outbox errors are explicit so a future transport never guesses delivery state.
#[derive(Debug, Error)]
pub enum OutboxError {
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
}

fn positive_sequence(sequence: u64) -> Result<i64, OutboxError> {
    let sequence = i64::try_from(sequence).map_err(|_| OutboxError::SequenceOutOfRange)?;
    if sequence <= 0 {
        return Err(OutboxError::SequenceMustBePositive);
    }
    Ok(sequence)
}

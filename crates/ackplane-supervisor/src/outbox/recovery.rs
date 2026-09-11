use rusqlite::{OptionalExtension, Transaction, TransactionBehavior};

use super::{OutboxError, QueuedFrame, SupervisorOutbox};

#[derive(Clone, Copy)]
pub(crate) struct RecoveryAttempt<'a> {
    pub marker_digest: &'a str,
    pub confirmation_digest: &'a str,
    pub reason: &'a str,
}

pub(crate) struct RecoveryResult {
    pub result_digest: String,
    pub server_position: u64,
    pub lease_released: bool,
}

pub(crate) struct RecoveryEvidence {
    pub stop: Option<(Vec<u8>, QueuedFrame)>,
    pub frames: Vec<QueuedFrame>,
    pub positions: super::OutboxPositions,
    pub pending_frames: usize,
    pub completed: Option<String>,
}

impl QueuedFrame {
    fn decode_exact((sequence, bytes): (u64, Vec<u8>)) -> Result<Self, OutboxError> {
        use prost::Message;
        let frame = Self::decode((sequence, bytes.clone()))?;
        if frame.frame.encode_to_vec() != bytes {
            return Err(OutboxError::RecoveryEvidence(
                "stored wire bytes cannot be replayed identically by this version".into(),
            ));
        }
        Ok(frame)
    }
}

impl SupervisorOutbox {
    pub(crate) fn recovery_snapshot(&self) -> Result<RecoveryEvidence, OutboxError> {
        use prost::Message;
        let _transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Deferred)?;
        let (count, size): (u64, u64) = self.conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(length(frame)), 0) FROM (SELECT frame FROM outbound_frames UNION ALL SELECT frame FROM acknowledged_lifecycle_receipts)",
            [], |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if count > 1024 || size > 4 * 1024 * 1024 {
            return Err(OutboxError::RecoveryEvidence(
                "history exceeds inspection limits".into(),
            ));
        }
        let positions = self.positions()?;
        let pending = crate::storage::pending_outbound_frames(&self.conn, 1025)?;
        let pending_frames = pending.len();
        if positions.last_enqueued.checked_sub(positions.acknowledged)
            != Some(pending_frames as u64)
            || pending.iter().enumerate().any(|(index, (sequence, _))| {
                *sequence != positions.acknowledged + index as u64 + 1
            })
        {
            return Err(OutboxError::RecoveryEvidence(
                "pending history has a sequence gap".into(),
            ));
        }
        let mut stored = crate::storage::acknowledged_lifecycle_receipts(&self.conn, 0, 1025)?;
        if stored
            .iter()
            .any(|(sequence, _)| *sequence == 0 || *sequence > positions.acknowledged)
        {
            return Err(OutboxError::RecoveryEvidence(
                "archive conflicts with acknowledgement position".into(),
            ));
        }
        stored.extend(pending);
        let frames = stored
            .into_iter()
            .map(QueuedFrame::decode_exact)
            .collect::<Result<Vec<_>, _>>()?;
        let stop = self.stopped_run()?;
        if let Some((_, stopped)) = &stop {
            if !frames.iter().any(|frame| {
                frame.sequence == stopped.sequence
                    && frame.frame.encode_to_vec() == stopped.frame.encode_to_vec()
            }) {
                return Err(OutboxError::RecoveryEvidence(
                    "stopped worker receipt is absent or changed".into(),
                ));
            }
        }
        let completed = self.conn.query_row("SELECT marker_digest FROM worker_recovery_events WHERE event = 'completed' ORDER BY rowid DESC LIMIT 1", [], |row| row.get(0)).optional()?;
        Ok(RecoveryEvidence {
            stop,
            frames,
            positions,
            pending_frames,
            completed,
        })
    }

    pub(crate) fn stopped_run(&self) -> Result<Option<(Vec<u8>, QueuedFrame)>, OutboxError> {
        Self::load_stopped_run(&self.conn)
    }

    pub(crate) fn archived_run_marker(path: &std::path::Path) -> Result<Vec<u8>, OutboxError> {
        let connection = rusqlite::Connection::open_with_flags(
            path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        Self::load_stopped_run(&connection)?
            .map(|(marker, _)| marker)
            .ok_or_else(|| {
                OutboxError::RecoveryEvidence(
                    "the requested run has no archived stop record".into(),
                )
            })
    }

    fn load_stopped_run(
        connection: &rusqlite::Connection,
    ) -> Result<Option<(Vec<u8>, QueuedFrame)>, OutboxError> {
        let record: Option<(Vec<u8>, u64, Vec<u8>)> = connection.query_row(
            "SELECT CASE WHEN length(marker) BETWEEN 1 AND 65536 THEN marker END, sequence,
             CASE WHEN length(frame) BETWEEN 1 AND 1048576 THEN frame END FROM stopped_worker_run WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        record
            .map(|(marker, sequence, bytes)| {
                Ok((marker, QueuedFrame::decode_exact((sequence, bytes))?))
            })
            .transpose()
    }

    pub(crate) fn record_recovery_event(
        &self,
        attempt: &RecoveryAttempt<'_>,
        result: Option<&RecoveryResult>,
    ) -> Result<(), OutboxError> {
        let event = if result.is_some() {
            "completed"
        } else {
            "started"
        };
        let transaction = Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)?;
        let existing: Option<String> = transaction.query_row(
            "SELECT marker_digest FROM worker_recovery_events WHERE event = ?1 AND confirmation_digest = ?2",
            [event, attempt.confirmation_digest], |row| row.get(0),
        ).optional()?;
        if let Some(existing) = existing {
            if existing != attempt.marker_digest {
                return Err(OutboxError::RecoveryEvidence(
                    "recovery event belongs to another marker".into(),
                ));
            }
        } else {
            let reason = if let Some(result) = result {
                let positions = self.positions()?;
                if positions.acknowledged != positions.last_enqueued
                    || result.server_position != positions.last_enqueued
                {
                    return Err(OutboxError::RecoveryEvidence(
                        "cleanup is not acknowledged through the final frame".into(),
                    ));
                }
                let started: Option<(String, String)> = transaction.query_row(
                    "SELECT marker_digest, reason FROM worker_recovery_events WHERE event = 'started' AND confirmation_digest = ?1",
                    [attempt.confirmation_digest], |row| Ok((row.get(0)?, row.get(1)?)),
                ).optional()?;
                match started {
                    Some((marker, reason)) if marker == attempt.marker_digest => reason,
                    _ => {
                        return Err(OutboxError::RecoveryEvidence(
                            "cleanup has no matching started attempt".into(),
                        ))
                    }
                }
            } else {
                attempt.reason.to_string()
            };
            let recorded_at = time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|_| {
                    OutboxError::RecoveryEvidence("recovery clock cannot be represented".into())
                })?;
            transaction.execute(
                "INSERT INTO worker_recovery_events (event, marker_digest, reason, recorded_at, confirmation_digest, result_digest, server_position, lease_released) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![event, attempt.marker_digest, reason, recorded_at, attempt.confirmation_digest,
                    result.map(|value| &value.result_digest), result.map(|value| value.server_position), result.map(|value| value.lease_released)],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn completed_recovery(
        &self,
        attempt: &RecoveryAttempt<'_>,
    ) -> Result<Option<RecoveryResult>, OutboxError> {
        let result = self.conn.query_row(
            "SELECT marker_digest, result_digest, server_position, lease_released FROM worker_recovery_events WHERE event = 'completed' AND confirmation_digest = ?1",
            [attempt.confirmation_digest], |row| Ok((row.get::<_, String>(0)?, RecoveryResult {
                result_digest: row.get(1)?, server_position: row.get(2)?, lease_released: row.get(3)?,
            })),
        ).optional()?;
        match result {
            Some((marker, result)) if marker == attempt.marker_digest => Ok(Some(result)),
            Some(_) => Err(OutboxError::RecoveryEvidence(
                "completed recovery belongs to another marker".into(),
            )),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ackplane_protocol::{supervisor::*, v1};
    use prost::Message;

    fn outbox() -> SupervisorOutbox {
        SupervisorOutbox::open_in_memory(
            SupervisorRegistration {
                supervisor_id: "supervisor".into(),
                identity: SupervisorIdentity {
                    tenant_id: "tenant".into(),
                    repository_id: "repository".into(),
                    node_id: "node".into(),
                },
                supervisor_version: "1".into(),
                protocol_version: "v1".into(),
                capabilities: SupervisorCapabilities {
                    supported_directives: vec![
                        SupervisorDirectiveCapability::Assign,
                        SupervisorDirectiveCapability::TerminateForce,
                    ],
                    supports_checkpoint: false,
                    supports_force_termination: true,
                    outbox_durability: SupervisorOutboxDurability::Persistent,
                    recoverable_outbox: true,
                },
            },
            SupervisorSession {
                session_id: "session".into(),
                supervisor_id: "supervisor".into(),
                worker_id: "worker".into(),
                runtime: SupervisorRuntime::LocalMachine,
                started_at: 1,
                state: SupervisorWorkerState::Started,
            },
        )
        .unwrap()
    }

    fn terminal() -> v1::NodeFrame {
        v1::NodeFrame {
            frame: Some(v1::node_frame::Frame::SupervisorLifecycleReceipt(
                v1::SupervisorLifecycleReceipt {
                    supervisor_id: "supervisor".into(),
                    session_id: "session".into(),
                    worker_id: "worker".into(),
                    occurred_at: "2026-09-11T00:00:00Z".into(),
                    state: v1::SupervisorWorkerState::Terminated as i32,
                    reason: v1::SupervisorLifecycleReason::Unspecified as i32,
                    idempotency_key: "terminal".into(),
                    outbox_sequence: None,
                },
            )),
        }
    }

    #[test]
    fn a_stop_record_and_its_terminal_frame_commit_together_and_survive_acknowledgement() {
        let outbox = outbox();
        let marker = b"original bounded run marker";
        let queued = outbox.enqueue_with_stop(terminal(), Some(marker)).unwrap();
        let (stored_marker, stored_frame) = outbox
            .stopped_run()
            .unwrap()
            .expect("a terminal frame alone is not stop provenance");
        assert_eq!(stored_marker, marker);
        assert_eq!(
            stored_frame.frame.encode_to_vec(),
            queued.frame.encode_to_vec()
        );
        assert_eq!(stored_frame.sequence, queued.sequence);
        outbox.acknowledge_through(queued.sequence).unwrap();
        assert_eq!(
            outbox.stopped_run().unwrap(),
            Some((marker.to_vec(), queued))
        );
    }

    #[test]
    fn failed_stop_record_does_not_leave_an_unproven_terminal_frame() {
        let outbox = outbox();
        outbox.conn.execute_batch("CREATE TRIGGER reject_stop BEFORE INSERT ON stopped_worker_run BEGIN SELECT RAISE(ABORT, 'injected stop record failure'); END;").unwrap();
        assert!(outbox.enqueue_with_stop(terminal(), Some(b"run")).is_err());
        assert!(outbox.pending(1).unwrap().is_empty());
        assert!(outbox.stopped_run().unwrap().is_none());
        assert_eq!(outbox.positions().unwrap().last_enqueued, 0);
    }

    #[test]
    fn recovery_events_are_idempotent_and_cannot_rebind_an_existing_attempt() {
        let outbox = outbox();
        let attempt = RecoveryAttempt {
            marker_digest: "digest",
            confirmation_digest: "preview",
            reason: "operator request",
        };
        let result = RecoveryResult {
            result_digest: "settled".into(),
            server_position: 1,
            lease_released: true,
        };
        outbox.enqueue_with_stop(terminal(), Some(b"run")).unwrap();
        outbox.record_recovery_event(&attempt, None).unwrap();
        outbox
            .record_recovery_event(
                &RecoveryAttempt {
                    reason: "retry reason",
                    ..attempt
                },
                None,
            )
            .unwrap();
        assert!(outbox
            .record_recovery_event(
                &RecoveryAttempt {
                    marker_digest: "changed",
                    ..attempt
                },
                None
            )
            .is_err());
        assert!(outbox.completed_recovery(&attempt).unwrap().is_none());
        assert!(outbox
            .record_recovery_event(&attempt, Some(&result))
            .is_err());
        outbox.acknowledge_through(1).unwrap();
        outbox
            .record_recovery_event(&attempt, Some(&result))
            .unwrap();
        assert!(outbox.completed_recovery(&attempt).unwrap().is_some());
        let reason: String = outbox
            .conn
            .query_row(
                "SELECT reason FROM worker_recovery_events WHERE event = 'started'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reason, "operator request");
    }
}

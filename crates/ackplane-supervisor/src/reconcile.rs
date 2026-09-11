//! Reconnect position reconciliation (ADR-0116 decision 7).
//!
//! On a fresh connection a supervisor must decide, from durable state alone,
//! whether it is resuming cleanly or has genuinely lost evidence. This is a
//! pure comparison deliberately: it needs no connection, no clock, and no
//! store, so the daemon supplies the acknowledged and enqueued local boundaries
//! plus the independently reported server position.
//!
//! The asymmetry between the two directions is the whole point, and it is easy
//! to get backwards:
//!
//! - A server position between acknowledged and last-enqueued is recoverable:
//!   every not-yet-acknowledged frame remains available for idempotent replay.
//!   Server acceptance without a received acknowledgement is an ordinary case.
//! - A server beyond last-enqueued holds frames lost locally. A server below
//!   acknowledged needs frames already pruned locally. Neither gap can be
//!   repaired by replaying the retained interval, so both require recovery.

use crate::outbox::OutboxPositions;

/// What a supervisor should do with a freshly opened connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconciliation {
    /// Local and server agree and nothing is outstanding.
    UpToDate { position: u64 },
    /// Replay retained frames from `resend_from` inclusive. Already-accepted
    /// frames are included when their acknowledgement was lost.
    Resend { resend_from: u64, through: u64 },
    /// One side needs evidence no longer retained by the other. Reported rather
    /// than repaired by assuming a position proves the missing frame content.
    IncompleteEvidence {
        /// Frames through this position were acknowledged and pruned locally.
        local_acknowledged: u64,
        /// The highest frame the local outbox has ever recorded.
        local_last_enqueued: u64,
        /// The server's position outside the locally recoverable interval.
        server_accepted: u64,
    },
}

impl Reconciliation {
    /// Whether work may resume without an operator decision.
    pub fn may_resume(self) -> bool {
        !matches!(self, Self::IncompleteEvidence { .. })
    }

    /// The gap this supervisor cannot describe, when there is one.
    pub fn missing_frames(self) -> Option<u64> {
        match self {
            Self::IncompleteEvidence {
                local_acknowledged,
                local_last_enqueued,
                server_accepted,
            } => Some(
                server_accepted.saturating_sub(local_last_enqueued)
                    + local_acknowledged.saturating_sub(server_accepted),
            ),
            _ => None,
        }
    }
}

/// Compare durable local progress against the position the server reports.
pub fn reconcile(local: OutboxPositions, server_accepted: u64) -> Reconciliation {
    if server_accepted > local.last_enqueued || server_accepted < local.acknowledged {
        return Reconciliation::IncompleteEvidence {
            local_acknowledged: local.acknowledged,
            local_last_enqueued: local.last_enqueued,
            server_accepted,
        };
    }
    if local.acknowledged == local.last_enqueued {
        return Reconciliation::UpToDate {
            position: server_accepted,
        };
    }
    Reconciliation::Resend {
        resend_from: local.acknowledged.saturating_add(1),
        through: local.last_enqueued,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn positions(acknowledged: u64, last_enqueued: u64) -> OutboxPositions {
        OutboxPositions {
            acknowledged,
            last_enqueued,
        }
    }

    #[test]
    fn agreement_with_nothing_queued_is_up_to_date() {
        assert_eq!(
            reconcile(positions(7, 7), 7),
            Reconciliation::UpToDate { position: 7 }
        );
    }

    #[test]
    fn a_fresh_supervisor_with_no_history_is_up_to_date() {
        assert_eq!(
            reconcile(positions(0, 0), 0),
            Reconciliation::UpToDate { position: 0 }
        );
    }

    /// The ordinary case an outbox exists for: frames queued while
    /// disconnected, resent from the server's position.
    #[test]
    fn a_server_behind_the_outbox_is_an_ordinary_resend() {
        assert_eq!(
            reconcile(positions(4, 9), 4),
            Reconciliation::Resend {
                resend_from: 5,
                through: 9
            }
        );
    }

    /// ADR-0116 decision 3: a supervisor must never pretend it persisted an
    /// event it did not publish. A server ahead of local durable state means
    /// frames were published that this supervisor can no longer describe, and
    /// resending from the server's position would hide exactly those frames
    /// behind a clean-looking resume.
    #[test]
    fn a_server_ahead_of_local_state_is_reported_not_silently_resumed() {
        let outcome = reconcile(positions(3, 3), 8);

        assert_eq!(
            outcome,
            Reconciliation::IncompleteEvidence {
                local_acknowledged: 3,
                local_last_enqueued: 3,
                server_accepted: 8
            }
        );
        assert!(!outcome.may_resume());
        assert_eq!(outcome.missing_frames(), Some(5));
    }

    /// A wiped local file is the sharpest form of the same fault: it reports
    /// position zero, which is a legitimate value for a new supervisor, so
    /// only the comparison against the server distinguishes "new" from "lost".
    #[test]
    fn a_wiped_outbox_against_a_server_with_history_is_incomplete_evidence() {
        let outcome = reconcile(positions(0, 0), 12);

        assert!(!outcome.may_resume());
        assert_eq!(outcome.missing_frames(), Some(12));
    }

    /// Local state ahead of the server while frames are still queued is the
    /// resend case, not a fault: the outbox is holding exactly the frames the
    /// server has not accepted yet.
    #[test]
    fn queued_frames_beyond_the_server_position_still_resume() {
        let outcome = reconcile(positions(4, 6), 4);

        assert!(outcome.may_resume());
        assert_eq!(outcome.missing_frames(), None);
    }

    #[test]
    fn a_clean_resume_reports_no_missing_frames() {
        assert_eq!(reconcile(positions(2, 2), 2).missing_frames(), None);
    }

    /// Shutdown can interrupt an acknowledgement after the server stores the frame.
    /// The retained frame is replayable, so this must not report lost evidence.
    #[test]
    fn a_lost_acknowledgement_replays_retained_frames() {
        assert_eq!(
            reconcile(positions(2, 4), 3),
            Reconciliation::Resend {
                resend_from: 3,
                through: 4
            }
        );
    }

    #[test]
    fn a_server_at_the_high_water_mark_still_confirms_pending_frames() {
        assert_eq!(
            reconcile(positions(2, 4), 4),
            Reconciliation::Resend {
                resend_from: 3,
                through: 4
            }
        );
    }

    #[test]
    fn missing_evidence_excludes_frames_still_retained_locally() {
        let outcome = reconcile(positions(2, 4), 7);
        assert!(!outcome.may_resume());
        assert_eq!(outcome.missing_frames(), Some(3));
    }

    /// A rolled-back server cannot be repaired by replaying already-pruned frames.
    #[test]
    fn a_server_behind_pruned_frames_needs_recovery_not_a_fictitious_resend() {
        let outcome = reconcile(positions(5, 7), 3);
        assert!(!outcome.may_resume());
        assert_eq!(outcome.missing_frames(), Some(2));
    }
}

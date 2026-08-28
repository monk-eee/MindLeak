//! Reconnect position reconciliation (ADR-0116 decision 7).
//!
//! On a fresh connection a supervisor must decide, from durable state alone,
//! whether it is resuming cleanly or has genuinely lost evidence. This is a
//! pure comparison deliberately: it needs no connection, no clock, and no
//! store, so the decision can be tested exhaustively and the daemon that will
//! wire it (slice 5) only has to supply two numbers.
//!
//! The asymmetry between the two directions is the whole point, and it is easy
//! to get backwards:
//!
//! - The server holding **less** than the supervisor is ordinary. It means
//!   frames were queued and not yet accepted, which is exactly what an outbox
//!   is for. The supervisor resends from the server's position.
//! - The server holding **more** than the supervisor is not recoverable by
//!   resending. It means the supervisor's own durable record is behind reality
//!   -- a rolled-back, restored, or truncated file -- so there are frames it
//!   published and can no longer describe. Resending from the server's position
//!   would look identical to a clean resume while quietly skipping them, which
//!   is precisely the "pretending it persisted an event it did not publish"
//!   that ADR-0116 decision 3 forbids. It is reported, never repaired by
//!   assumption.

use crate::outbox::OutboxPositions;

/// What a supervisor should do with a freshly opened connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconciliation {
    /// Local and server agree and nothing is outstanding.
    UpToDate { position: u64 },
    /// The server is behind the local outbox: resend from `resend_from`
    /// inclusive. Ordinary catch-up, not a fault.
    Resend { resend_from: u64, through: u64 },
    /// The server holds evidence this supervisor cannot account for. Reported
    /// rather than resolved: only an operator (or slice 5's daemon policy) can
    /// decide whether to adopt the server's position or investigate.
    IncompleteEvidence {
        /// The highest sequence local durable state can prove.
        local_acknowledged: u64,
        /// The higher position the server reports having accepted.
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
                server_accepted,
            } => Some(server_accepted.saturating_sub(local_acknowledged)),
            _ => None,
        }
    }
}

/// Compare durable local progress against the position the server reports.
pub fn reconcile(local: OutboxPositions, server_accepted: u64) -> Reconciliation {
    if server_accepted > local.acknowledged {
        return Reconciliation::IncompleteEvidence {
            local_acknowledged: local.acknowledged,
            server_accepted,
        };
    }
    if server_accepted == local.last_enqueued {
        return Reconciliation::UpToDate {
            position: server_accepted,
        };
    }
    Reconciliation::Resend {
        resend_from: server_accepted.saturating_add(1),
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
}

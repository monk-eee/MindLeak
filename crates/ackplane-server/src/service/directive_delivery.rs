//! Live delivery of undelivered directives to an authenticated supervisor
//! session (ADR-0116 slice 3, closing ADR-0107's control loop).
//!
//! Delivery is attached to the `SupervisorSession` frame rather than to the
//! heartbeat, for a reason that is load-bearing rather than incidental: a
//! heartbeat names only its supervisor, while directives are addressed to a
//! `(node, agent_session)` pair, and inventing a session for an unaddressed
//! heartbeat would mean guessing the target. The session frame states that
//! pair outright, and it is also exactly what a reconnecting supervisor
//! re-sends — so reconnect redelivery falls out of the same path instead of
//! needing a second one.
//!
//! Directives are emitted *ahead* of the session frame's own receipt, which
//! makes that receipt a delivery barrier: a client reading until its receipt
//! has necessarily already seen every directive delivered with it. Sending
//! them afterwards leaves the client waiting on frames that may never come,
//! and a non-blocking drain then misses directives still in flight — measured,
//! not theorised: this delivery path was written that way first and its
//! end-to-end test failed on exactly that race.
//!
//! Delivery is at-least-once and says so. Ackplane records no `delivered_at`,
//! because a frame it put on a stream is not evidence a supervisor acted
//! (ADR-0107); only the returned receipt is. A redelivered directive is
//! therefore normal, and `SupervisorInbox::receive` answers it by replaying
//! the original receipt rather than acting twice.

use ackplane_protocol::v1;

use crate::directive_store::DirectiveStore;

/// The supervisor session a frame addresses, when it names one.
///
/// Only `SupervisorSession` does. A registration has no session yet, and a
/// heartbeat and lifecycle receipt do not identify the directive target
/// unambiguously.
pub(super) fn addressed_session(frame: &v1::NodeFrame) -> Option<String> {
    match &frame.frame {
        Some(v1::node_frame::Frame::SupervisorSession(wire))
            if !wire.session_id.is_empty() && !wire.supervisor_id.is_empty() =>
        {
            Some(wire.session_id.clone())
        }
        _ => None,
    }
}

/// Frames carrying every directive this session still owes a receipt for.
///
/// A read failure yields no frames rather than a rejection: the supervisor
/// frame that triggered this was itself accepted and already has its receipt,
/// and failing it afterwards would tell the supervisor its session did not
/// register when it did. The directives stay pending and are delivered on the
/// next session frame.
pub(super) async fn pending_frames(
    directives: &DirectiveStore,
    tenant_id: &str,
    repository_id: &str,
    node_id: &str,
    agent_session_id: &str,
) -> Vec<v1::AckplaneFrame> {
    match directives
        .pending_for_session(
            tenant_id,
            repository_id,
            node_id,
            agent_session_id,
            crate::directive_store::MAX_DELIVERY_BATCH,
        )
        .await
    {
        Ok(directives) => directives
            .into_iter()
            .map(|directive| v1::AckplaneFrame {
                frame: Some(v1::ackplane_frame::Frame::AgentDirective(Box::new(
                    directive,
                ))),
            })
            .collect(),
        Err(error) => {
            tracing::error!(%error, "pending directive delivery query failed");
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_frame(supervisor_id: &str, session_id: &str) -> v1::NodeFrame {
        v1::NodeFrame {
            frame: Some(v1::node_frame::Frame::SupervisorSession(
                v1::SupervisorSession {
                    supervisor_id: supervisor_id.to_string(),
                    session_id: session_id.to_string(),
                    ..Default::default()
                },
            )),
        }
    }

    #[test]
    fn a_session_frame_names_the_directive_target() {
        assert_eq!(
            addressed_session(&session_frame("supervisor-1", "session-1")),
            Some("session-1".to_string())
        );
    }

    /// A heartbeat names only its supervisor. Treating it as a delivery
    /// trigger would mean choosing a session on the supervisor's behalf, and
    /// a directive delivered to a session it was not addressed to is refused
    /// by the receiving inbox anyway -- so the guess buys nothing and hides
    /// the addressing bug behind a refusal.
    #[test]
    fn a_heartbeat_does_not_name_a_session_so_it_triggers_no_delivery() {
        let heartbeat = v1::NodeFrame {
            frame: Some(v1::node_frame::Frame::SupervisorHeartbeat(
                v1::SupervisorHeartbeat {
                    supervisor_id: "supervisor-1".to_string(),
                },
            )),
        };

        assert_eq!(addressed_session(&heartbeat), None);
    }

    #[test]
    fn an_incomplete_session_frame_addresses_nothing() {
        assert_eq!(addressed_session(&session_frame("supervisor-1", "")), None);
        assert_eq!(addressed_session(&session_frame("", "session-1")), None);
    }
}

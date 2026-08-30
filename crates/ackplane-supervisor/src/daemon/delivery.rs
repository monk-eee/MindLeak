//! Getting durable outbox frames onto the connection, and back off it.
//!
//! Split from `daemon/mod.rs`, which owns the connection lifecycle. This owns
//! the narrower question of how a receipt moves between the durable outbox and
//! Ackplane: what position it is stamped with on the way out, and what happens
//! to frames the previous connection never got confirmed.

use ackplane_client::{node_sync::NodeSyncConnection, ClientError};
use ackplane_protocol::v1;

use super::{DaemonError, DaemonExit};
use crate::SupervisorOutbox;

/// How many unconfirmed frames one reconnect may resend before serving new
/// work. Bounded so a long backlog is drained in instalments rather than one
/// burst that could outrun the connection's flow control.
const MAX_RESEND_BATCH: u32 = 32;

/// Queue one directive receipt durably and return the sequence it took, so the
/// caller can acknowledge exactly that frame once the server confirms it.
///
/// The receipt is stamped with that sequence (ADR-0146 decision 1) *before* it
/// is stored, and the stamped copy is handed back for transmission, so the
/// durable copy and the transmitted copy are byte-identical.
///
/// That is not tidiness. The server keys a receipt's idempotency on
/// `SHA-256(receipt.encode_to_vec())`, so a resent copy differing by this one
/// field would hash differently and be recorded as a second, distinct receipt
/// instead of being recognised as the replay it is -- turning the durable
/// outbox from a safeguard into a source of duplicates.
pub(super) fn enqueue_receipt(
    outbox: &SupervisorOutbox,
    mut receipt: v1::DirectiveReceipt,
) -> Result<(u64, v1::DirectiveReceipt), DaemonError> {
    let sequence = outbox.positions()?.last_enqueued.saturating_add(1);
    receipt.outbox_sequence = Some(sequence);
    outbox.enqueue(
        sequence,
        &v1::NodeFrame {
            frame: Some(v1::node_frame::Frame::DirectiveReceipt(receipt.clone())),
        },
    )?;
    Ok((sequence, receipt))
}

/// Resend every frame this supervisor queued but never got confirmed.
///
/// Public so the behaviour can be driven directly against a real connection in
/// a test: `serve_once` runs an unbounded serve loop, so a test that called it
/// would have to decide when to stop it, which tests the harness rather than
/// the resend.
///
/// This is the whole reason the outbox is durable. A receipt that was computed,
/// written down, and then lost to a dropped connection is re-sent from local
/// state here rather than depending on the server to redeliver its directive.
/// Redelivery does also cover it today, but that is a guarantee held by the
/// other side of the connection that just failed, and the inbox replays an
/// identical receipt for a repeated directive either way -- so resending is
/// idempotent, never a double-report.
pub async fn resend_pending(
    outbox: &SupervisorOutbox,
    connection: &mut NodeSyncConnection,
) -> Result<Option<DaemonExit>, DaemonError> {
    let pending = outbox.pending(MAX_RESEND_BATCH)?;
    if pending.is_empty() {
        return Ok(None);
    }
    tracing::info!(
        count = pending.len(),
        "resending supervisor frames that were never confirmed"
    );
    for queued in pending {
        let sequence = queued.sequence;
        match connection.exchange_supervisor_frame(queued.frame).await {
            Ok(_) => {
                outbox.acknowledge_through(sequence)?;
            }
            // A frame the server refuses outright will be refused again on
            // every reconnect. Retrying it forever would wedge the daemon in a
            // reconnect loop and block every later frame behind it, so it is
            // dropped from the queue -- loudly, because a receipt Ackplane
            // will not accept is a real problem, just not one more attempts
            // can fix. `retryable` is the server's own judgement of which case
            // this is, so it decides rather than this code guessing.
            Err(ClientError::FrameRefused {
                reason,
                retryable: false,
                diagnostic,
            }) => {
                tracing::error!(
                    sequence,
                    ?reason,
                    %diagnostic,
                    "Ackplane permanently refused a queued supervisor frame; dropping it \
                     rather than resending it forever"
                );
                outbox.acknowledge_through(sequence)?;
            }
            Err(error) => {
                tracing::info!(%error, "the supervisor connection closed while resending");
                return Ok(Some(DaemonExit::Disconnected));
            }
        }
    }
    Ok(None)
}

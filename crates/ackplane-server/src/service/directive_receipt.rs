//! Authenticated ingestion and acknowledgement of typed directive receipts.

use ackplane_protocol::v1;

use crate::{
    directive_store::{DirectiveReceiptOutcome, DirectiveStore, DirectiveStoreError},
    supervisor_store::SupervisorStore,
};

pub(super) fn is_directive_receipt_frame(frame: &v1::NodeFrame) -> bool {
    matches!(
        frame.frame,
        Some(v1::node_frame::Frame::DirectiveReceipt(_))
    )
}

pub(super) fn unavailable_frame() -> v1::AckplaneFrame {
    rejection(IngressError::Unavailable(
        "directive receipt ingestion is not configured for this NodeSync service",
    ))
}

/// One durably accepted receipt, plus the outbox position the frame that just
/// arrived declared for it.
///
/// The declared sequence is kept separately rather than read back off
/// `outcome.record.receipt` because a replay returns the *stored* payload --
/// the copy from the first time this decision was seen, carrying that
/// occasion's sequence. When Ackplane redelivers a directive the supervisor
/// replays an identical decision from a new outbox slot, so the stored copy's
/// sequence is stale by exactly the amount the supervisor has moved on. Using
/// it would leave the server permanently one or more positions behind a
/// supervisor that is fully caught up, and every later reconnect would report
/// a resend that has nothing to resend.
#[derive(Debug)]
pub(super) struct AcceptedReceipt {
    pub(super) outcome: DirectiveReceiptOutcome,
    pub(super) declared_outbox_sequence: Option<u64>,
}

pub(super) async fn record_authenticated_receipt(
    frame: v1::NodeFrame,
    tenant_id: &str,
    repository_id: &str,
    node_id: &str,
    directives: &mut DirectiveStore,
) -> Result<AcceptedReceipt, IngressError> {
    let Some(v1::node_frame::Frame::DirectiveReceipt(receipt)) = frame.frame else {
        return Err(IngressError::Malformed("expected a directive receipt"));
    };
    if receipt.tenant_id != tenant_id
        || receipt.repository_id != repository_id
        || receipt.node_id != node_id
    {
        return Err(IngressError::Unauthorized(
            "directive receipt identity does not match the authenticated connection",
        ));
    }
    let declared_outbox_sequence = receipt.outbox_sequence;
    let outcome = directives
        .record_receipt(receipt)
        .await
        .map_err(IngressError::from)?;
    Ok(AcceptedReceipt {
        outcome,
        declared_outbox_sequence,
    })
}

pub(super) async fn acknowledgement(
    accepted: AcceptedReceipt,
    tenant_id: &str,
    repository_id: &str,
    supervisors: &SupervisorStore,
) -> Result<v1::AckplaneFrame, IngressError> {
    let AcceptedReceipt {
        outcome,
        declared_outbox_sequence,
    } = accepted;
    let receipt = &outcome.record.receipt;
    let session = supervisors
        .session(tenant_id, repository_id, &receipt.agent_session_id)
        .await
        .map_err(|_| IngressError::Unavailable("supervisor session lookup is unavailable"))?
        .ok_or(IngressError::Unauthorized(
            "directive receipt is not authorized for this authenticated connection",
        ))?;
    let supervisor_id = session.session.supervisor_id;
    // ADR-0146 decision 3. This runs only after `record_receipt` committed, so
    // the position advances on durable acceptance and never on a frame that
    // was merely received. A frame carrying no sequence advances nothing --
    // `None` here is not "zero", it is "this frame was not part of the
    // resendable outbox stream" (decision 2), and inferring a position for it
    // would be the server inventing evidence.
    //
    // A replay advances the position too, and deliberately: the supervisor
    // asserted this sequence on the frame it just sent, and
    // `record_outbox_sequence` only ever moves upward, so this is idempotent
    // for a true resend and correct for a redelivered directive receipted
    // again from a later outbox slot.
    let accepted_outbox_sequence = match declared_outbox_sequence {
        Some(sequence) => Some(
            supervisors
                .record_outbox_sequence(tenant_id, repository_id, &supervisor_id, sequence)
                .await
                .map_err(|_| {
                    IngressError::Unavailable("supervisor outbox position store is unavailable")
                })?,
        ),
        None => None,
    };
    tracing::info!(
        method = "Synchronize",
        operation = "directive_receipt",
        receipt_position = outcome.record.receipt_position,
        idempotent_replay = outcome.idempotent_replay,
        declared_outbox_sequence,
        "recorded directive receipt"
    );
    Ok(v1::AckplaneFrame {
        frame: Some(v1::ackplane_frame::Frame::SupervisorFrameReceipt(
            v1::SupervisorFrameReceipt {
                operation: v1::SupervisorFrameOperation::DirectiveReceipt as i32,
                supervisor_id,
                session_id: receipt.agent_session_id.clone(),
                idempotent_replay: outcome.idempotent_replay,
                projection_advanced: !outcome.idempotent_replay,
                accepted_outbox_sequence,
            },
        )),
    })
}

pub(super) fn rejection(error: IngressError) -> v1::AckplaneFrame {
    let (reason, retryable, diagnostic) = match error {
        IngressError::Malformed(diagnostic) => (v1::RejectionReason::Malformed, false, diagnostic),
        IngressError::Unauthorized(diagnostic) => {
            (v1::RejectionReason::Unauthorized, false, diagnostic)
        }
        IngressError::Unavailable(diagnostic) => {
            (v1::RejectionReason::Unavailable, true, diagnostic)
        }
    };
    tracing::warn!(
        method = "Synchronize",
        operation = "directive_receipt",
        reason = ?reason,
        retryable,
        "refused directive receipt"
    );
    v1::AckplaneFrame {
        frame: Some(v1::ackplane_frame::Frame::Rejection(v1::Rejection {
            record_id: String::new(),
            reason: reason as i32,
            retryable,
            diagnostic: diagnostic.to_string(),
        })),
    }
}

#[derive(Debug)]
pub(super) enum IngressError {
    Malformed(&'static str),
    Unauthorized(&'static str),
    Unavailable(&'static str),
}

impl From<DirectiveStoreError> for IngressError {
    fn from(error: DirectiveStoreError) -> Self {
        match error {
            DirectiveStoreError::Database(_) => {
                Self::Unavailable("directive receipt ledger is temporarily unavailable")
            }
            DirectiveStoreError::UnknownDirective | DirectiveStoreError::ReceiptMismatch => {
                Self::Unauthorized(
                    "directive receipt is not authorized for this authenticated connection",
                )
            }
            _ => Self::Malformed("directive receipt is invalid"),
        }
    }
}

#[cfg(test)]
mod tests;

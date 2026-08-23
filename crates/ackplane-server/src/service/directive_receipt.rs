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

pub(super) async fn record_authenticated_receipt(
    frame: v1::NodeFrame,
    tenant_id: &str,
    repository_id: &str,
    node_id: &str,
    directives: &mut DirectiveStore,
) -> Result<DirectiveReceiptOutcome, IngressError> {
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
    directives
        .record_receipt(receipt)
        .await
        .map_err(IngressError::from)
}

pub(super) async fn acknowledgement(
    outcome: DirectiveReceiptOutcome,
    tenant_id: &str,
    repository_id: &str,
    supervisors: &SupervisorStore,
) -> Result<v1::AckplaneFrame, IngressError> {
    let receipt = &outcome.record.receipt;
    let session = supervisors
        .session(tenant_id, repository_id, &receipt.agent_session_id)
        .await
        .map_err(|_| IngressError::Unavailable("supervisor session lookup is unavailable"))?
        .ok_or(IngressError::Unauthorized(
            "directive receipt is not authorized for this authenticated connection",
        ))?;
    tracing::info!(
        method = "Synchronize",
        operation = "directive_receipt",
        receipt_position = outcome.record.receipt_position,
        idempotent_replay = outcome.idempotent_replay,
        "recorded directive receipt"
    );
    Ok(v1::AckplaneFrame {
        frame: Some(v1::ackplane_frame::Frame::SupervisorFrameReceipt(
            v1::SupervisorFrameReceipt {
                operation: v1::SupervisorFrameOperation::DirectiveReceipt as i32,
                supervisor_id: session.session.supervisor_id,
                session_id: receipt.agent_session_id.clone(),
                idempotent_replay: outcome.idempotent_replay,
                projection_advanced: !outcome.idempotent_replay,
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

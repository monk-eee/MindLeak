//! Per-connection frame dispatch: advances the ADR-0098 handshake state
//! machine and processes authenticated event batches and heartbeats.

use std::future::Future;

use ackplane_protocol::v1;

use super::handshake::{connection_refused, handle_challenge_response, handle_hello};
use super::{ConnectionState, OwnedEnvelopeBinding};
use crate::{
    envelope_signature::{self, SignatureRefusal},
    ledger::{AppendError, AppendOutcome, EventEnvelope},
    signing_keys::{KeyResolution, SigningKeyError},
    sync,
};

/// Refuse a record whose sender could not be established.
///
/// Non-retryable throughout: every one of these is a property of the bytes and
/// the enrolment, so resending them unchanged fails identically.
fn unauthenticated(key: &crate::ledger::DedupKey, refusal: SignatureRefusal) -> v1::Rejection {
    v1::Rejection {
        record_id: sync::record_identity(key),
        reason: refusal.reason() as i32,
        retryable: false,
        diagnostic: refusal.diagnostic().to_string(),
    }
}

/// Whether the connection that produced these frames must be closed. A
/// revoked node must not be left free to keep sending records that will all
/// be refused the same way (ADR-0085 decision 8) — every other rejection
/// reason is retried or corrected by the sender on the same connection.
pub(super) fn must_terminate_after(frames: &[v1::AckplaneFrame]) -> bool {
    frames.iter().any(|frame| {
        matches!(
            &frame.frame,
            Some(v1::ackplane_frame::Frame::Rejection(rejection))
                if rejection.reason == v1::RejectionReason::NodeRevoked as i32
        )
    })
}

/// Process one node frame against this connection's handshake state, with the
/// supplied durable append operation.
///
/// The transport supplies the real ledger append and key lookup; tests supply
/// deterministic ones, so protocol decisions stay testable without PostgreSQL.
pub(super) async fn handle_frame<F, Fut, R, RFut>(
    frame: v1::NodeFrame,
    flow_control: v1::FlowControl,
    state: &mut ConnectionState,
    mut append: F,
    mut resolve_key: R,
) -> Vec<v1::AckplaneFrame>
where
    F: FnMut(EventEnvelope) -> Fut,
    Fut: Future<Output = Result<AppendOutcome, AppendError>>,
    R: FnMut(OwnedEnvelopeBinding) -> RFut,
    RFut: Future<Output = Result<KeyResolution, SigningKeyError>>,
{
    let Some(inner) = frame.frame else {
        return Vec::new();
    };

    let current = std::mem::replace(state, ConnectionState::Rejected);
    let (next_state, responses) = match (current, inner) {
        (ConnectionState::AwaitingHello, v1::node_frame::Frame::Hello(hello)) => {
            handle_hello(hello)
        }
        (ConnectionState::AwaitingHello, _) => (
            ConnectionState::Rejected,
            vec![connection_refused("a connection must send Hello first")],
        ),
        (
            ConnectionState::AwaitingChallengeResponse {
                nonce,
                tenant_id,
                repository_id,
                producer_id,
                signing_key_id,
                last_accepted_position,
            },
            v1::node_frame::Frame::ChallengeResponse(response),
        ) => {
            handle_challenge_response(
                response,
                nonce,
                tenant_id,
                repository_id,
                producer_id,
                signing_key_id,
                last_accepted_position,
                flow_control,
                &mut resolve_key,
            )
            .await
        }
        (ConnectionState::AwaitingChallengeResponse { .. }, _) => (
            ConnectionState::Rejected,
            vec![connection_refused(
                "expected a ChallengeResponse to the issued connection challenge",
            )],
        ),
        (authenticated @ ConnectionState::Authenticated { .. }, v1::node_frame::Frame::Heartbeat(_)) => {
            (authenticated, Vec::new())
        }
        (
            ConnectionState::Authenticated {
                tenant_id,
                repository_id,
                producer_id,
            },
            v1::node_frame::Frame::EventBatch(frame),
        ) => {
            let batch_started = std::time::Instant::now();
            let batch_records = frame.events.len() as u64;
            let batch_bytes: u64 = frame
                .events
                .iter()
                .map(|event| event.payload.len() as u64)
                .sum();
            let mut retry_count = 0u64;
            let mut position = None;

            let mut receipts = Vec::with_capacity(frame.events.len());
            let responses = 'batch: {
                for wire in frame.events {
                    let envelope = match sync::translate(&wire) {
                        Ok(envelope) => envelope,
                        Err(rejection) => {
                            break 'batch vec![v1::AckplaneFrame {
                                frame: Some(v1::ackplane_frame::Frame::Rejection(rejection)),
                            }];
                        }
                    };
                    let key = envelope.key.clone();

                    // Judged as of now, because now is when this record is being accepted.
                    // A key revoked after this instant must not retroactively invalidate
                    // what it signed (ADR-0084 decision 12).
                    let resolution = match resolve_key(OwnedEnvelopeBinding {
                        signing_key_id: wire.signing_key_id.clone(),
                        tenant_id: key.tenant_id.clone(),
                        repository_id: key.repository_id.clone(),
                        producer_id: key.producer_id.clone(),
                        accepted_at: std::time::SystemTime::now(),
                    })
                    .await
                    {
                        Ok(resolution) => resolution,
                        // A key store that cannot answer is not a node that cannot
                        // authenticate. Refusing this as unauthenticated would be
                        // non-retryable, permanently rejecting a record whose sender is
                        // very likely legitimate.
                        Err(error) => {
                            tracing::error!(%error, record = %sync::record_identity(&key), "signing key lookup failed");
                            break 'batch vec![v1::AckplaneFrame {
                                frame: Some(v1::ackplane_frame::Frame::Rejection(v1::Rejection {
                                    record_id: sync::record_identity(&key),
                                    reason: v1::RejectionReason::Unavailable as i32,
                                    retryable: true,
                                    diagnostic: "the key registry was unavailable; retry with backoff"
                                        .to_string(),
                                })),
                            }];
                        }
                    };

                    if let Err(refusal) =
                        envelope_signature::verify(&wire, envelope.provenance, &resolution)
                    {
                        break 'batch vec![v1::AckplaneFrame {
                            frame: Some(v1::ackplane_frame::Frame::Rejection(unauthenticated(
                                &key, refusal,
                            ))),
                        }];
                    }

                    match append(envelope).await {
                        Ok(outcome) => {
                            if matches!(outcome, AppendOutcome::Duplicate { .. }) {
                                retry_count += 1;
                            }
                            let receipt = sync::receipt(&key, outcome);
                            position = Some(receipt.position);
                            receipts.push(receipt);
                        }
                        Err(error) => {
                            break 'batch vec![v1::AckplaneFrame {
                                frame: Some(v1::ackplane_frame::Frame::Rejection(sync::rejection(
                                    &key, &error,
                                ))),
                            }];
                        }
                    }
                }

                vec![v1::AckplaneFrame {
                    frame: Some(v1::ackplane_frame::Frame::BatchReceipt(v1::BatchReceipt {
                        receipts,
                    })),
                }]
            };

            record_batch_observability(
                &responses,
                batch_started.elapsed(),
                batch_records,
                batch_bytes,
                retry_count,
                position,
            );

            (
                ConnectionState::Authenticated {
                    tenant_id,
                    repository_id,
                    producer_id,
                },
                responses,
            )
        }
        (ConnectionState::Authenticated { .. }, _) => (
            ConnectionState::Rejected,
            vec![connection_refused(
                "Hello and the connection challenge are only valid once, at the start of the connection",
            )],
        ),
        (ConnectionState::Rejected, _) => (ConnectionState::Rejected, Vec::new()),
    };
    *state = next_state;
    responses
}

/// ADR-0083 decision 10: both ends record method, outcome, reason code,
/// latency, batch count, byte count, retry count, and accepted position —
/// never the payload, signing material, or evidence body content that
/// travelled inside the batch.
fn record_batch_observability(
    responses: &[v1::AckplaneFrame],
    latency: std::time::Duration,
    batch_records: u64,
    batch_bytes: u64,
    retry_count: u64,
    position: Option<u64>,
) {
    let rejection = responses.iter().find_map(|frame| match &frame.frame {
        Some(v1::ackplane_frame::Frame::Rejection(rejection)) => Some(rejection),
        _ => None,
    });
    match rejection {
        Some(rejection) => {
            let reason = v1::RejectionReason::try_from(rejection.reason)
                .map(|reason| reason.as_str_name())
                .unwrap_or("UNKNOWN");
            tracing::info!(
                method = "Synchronize",
                outcome = "rejected",
                reason,
                retryable = rejection.retryable,
                latency_ms = latency.as_millis() as u64,
                batch_records,
                batch_bytes,
                retry_count,
                position,
                "processed event batch"
            );
        }
        None => {
            tracing::info!(
                method = "Synchronize",
                outcome = "accepted",
                latency_ms = latency.as_millis() as u64,
                batch_records,
                batch_bytes,
                retry_count,
                position,
                "processed event batch"
            );
        }
    }
}

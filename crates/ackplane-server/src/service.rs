//! gRPC transport for the node-to-ledger synchronization stream (ADR-0083).

use std::{future::Future, pin::Pin, sync::Arc, time::SystemTime};

use ackplane_protocol::v1;
use tokio::sync::{mpsc, Mutex};
use tokio_stream::{wrappers::ReceiverStream, Stream};
use tonic::{Request, Response, Status};

use crate::{
    envelope_signature::{self, SignatureRefusal},
    ledger::{AppendError, AppendOutcome, EventEnvelope, LedgerStore},
    signing_keys::{EnvelopeBinding, KeyResolution, SigningKeyError},
    sync,
};

/// What an envelope claims about which key signed it, owned rather than
/// borrowed so it can cross the injected lookup's async boundary.
#[derive(Debug, Clone)]
pub struct OwnedEnvelopeBinding {
    pub signing_key_id: String,
    pub tenant_id: String,
    pub repository_id: String,
    pub producer_id: String,
    pub accepted_at: SystemTime,
}

impl OwnedEnvelopeBinding {
    pub fn as_binding(&self) -> EnvelopeBinding<'_> {
        EnvelopeBinding {
            signing_key_id: &self.signing_key_id,
            tenant_id: &self.tenant_id,
            repository_id: &self.repository_id,
            producer_id: &self.producer_id,
            accepted_at: self.accepted_at,
        }
    }
}

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
fn must_terminate_after(frames: &[v1::AckplaneFrame]) -> bool {
    frames.iter().any(|frame| {
        matches!(
            &frame.frame,
            Some(v1::ackplane_frame::Frame::Rejection(rejection))
                if rejection.reason == v1::RejectionReason::NodeRevoked as i32
        )
    })
}

/// Process one node frame with the supplied durable append operation.
///
/// The transport supplies the real ledger append and key lookup; tests supply
/// deterministic ones, so protocol decisions stay testable without PostgreSQL.
pub async fn handle_frame<F, Fut, R, RFut>(
    frame: v1::NodeFrame,
    flow_control: v1::FlowControl,
    mut append: F,
    mut resolve_key: R,
) -> Vec<v1::AckplaneFrame>
where
    F: FnMut(EventEnvelope) -> Fut,
    Fut: Future<Output = Result<AppendOutcome, AppendError>>,
    R: FnMut(OwnedEnvelopeBinding) -> RFut,
    RFut: Future<Output = Result<KeyResolution, SigningKeyError>>,
{
    let Some(frame) = frame.frame else {
        return Vec::new();
    };
    let frame = match frame {
        v1::node_frame::Frame::Hello(hello) => {
            return vec![
                v1::AckplaneFrame {
                    frame: Some(v1::ackplane_frame::Frame::HelloAccepted(
                        v1::HelloAccepted {
                            accepted_position: hello.last_accepted_position,
                            // A node's requested capabilities are not proof that
                            // the server enables them. Advertise only capabilities
                            // this transport has explicitly selected.
                            enabled_capabilities: Vec::new(),
                        },
                    )),
                },
                v1::AckplaneFrame {
                    frame: Some(v1::ackplane_frame::Frame::FlowControl(flow_control)),
                },
            ];
        }
        v1::node_frame::Frame::Heartbeat(_) => return Vec::new(),
        v1::node_frame::Frame::EventBatch(batch) => batch,
    };

    let mut receipts = Vec::with_capacity(frame.events.len());
    for wire in frame.events {
        let envelope = match sync::translate(&wire) {
            Ok(envelope) => envelope,
            Err(rejection) => {
                return vec![v1::AckplaneFrame {
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
            accepted_at: SystemTime::now(),
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
                return vec![v1::AckplaneFrame {
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

        if let Err(refusal) = envelope_signature::verify(&wire, envelope.provenance, &resolution) {
            return vec![v1::AckplaneFrame {
                frame: Some(v1::ackplane_frame::Frame::Rejection(unauthenticated(
                    &key, refusal,
                ))),
            }];
        }

        match append(envelope).await {
            Ok(outcome) => receipts.push(sync::receipt(&key, outcome)),
            Err(error) => {
                return vec![v1::AckplaneFrame {
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
}

/// The concrete gRPC service. The ledger is deliberately serialized: its
/// append transaction borrows the PostgreSQL client mutably, and ordering on a
/// stream is part of the receipt contract.
pub struct NodeSyncService {
    ledger: Arc<Mutex<LedgerStore>>,
    flow_control: v1::FlowControl,
}

impl NodeSyncService {
    pub fn new(ledger: LedgerStore, flow_control: v1::FlowControl) -> Self {
        Self {
            ledger: Arc::new(Mutex::new(ledger)),
            flow_control,
        }
    }
}

#[tonic::async_trait]
impl v1::node_sync_service_server::NodeSyncService for NodeSyncService {
    type SynchronizeStream = Pin<Box<dyn Stream<Item = Result<v1::AckplaneFrame, Status>> + Send>>;

    async fn synchronize(
        &self,
        request: Request<tonic::Streaming<v1::NodeFrame>>,
    ) -> Result<Response<Self::SynchronizeStream>, Status> {
        let ledger = Arc::clone(&self.ledger);
        let flow_control = self.flow_control;
        let (sender, receiver) = mpsc::channel(8);

        tokio::spawn(async move {
            let mut inbound = request.into_inner();
            'stream: loop {
                match inbound.message().await {
                    Ok(Some(frame)) => {
                        let responses = handle_frame(
                            frame,
                            flow_control,
                            |envelope| {
                                let ledger = Arc::clone(&ledger);
                                async move { ledger.lock().await.append(&envelope).await }
                            },
                            |binding| {
                                let ledger = Arc::clone(&ledger);
                                async move {
                                    ledger
                                        .lock()
                                        .await
                                        .resolve_signing_key(&binding.as_binding())
                                        .await
                                }
                            },
                        )
                        .await;
                        let terminate = must_terminate_after(&responses);
                        for frame in responses {
                            if sender.send(Ok(frame)).await.is_err() {
                                break 'stream;
                            }
                        }
                        if terminate {
                            break 'stream;
                        }
                    }
                    Ok(None) => break,
                    Err(error) => {
                        let _ = sender.send(Err(error)).await;
                        break;
                    }
                }
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(receiver))))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        future::ready,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };

    use ed25519_dalek::{Signer, SigningKey};
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::{
        enrollment::public_key_fingerprint,
        envelope_signature::envelope_signing_bytes,
        signing_keys::{SigningKeyLifecycle, SigningKeyRecord},
    };

    /// An unsigned envelope claiming only what an unsigned envelope can claim.
    ///
    /// These tests exercise append, receipts and conflicts, not authentication.
    /// Declaring `EnrolledNode` here without a signature would now be refused
    /// at the trust boundary before any of that ran — correctly, which is why
    /// the fixture says what it actually is.
    fn envelope(payload: &[u8]) -> v1::EventEnvelope {
        v1::EventEnvelope {
            tenant_id: "acme".to_string(),
            repository_id: "repo".to_string(),
            producer_id: "node-1".to_string(),
            producer_sequence: 7,
            payload: payload.to_vec(),
            payload_digest: Sha256::digest(payload).to_vec(),
            schema_version: "v1".to_string(),
            occurred_at: "2026-08-13T00:00:00Z".to_string(),
            payload_type: "structural_fact".to_string(),
            previous_envelope_digest: Vec::new(),
            signing_key_id: String::new(),
            signature: Vec::new(),
            provenance: v1::ProvenanceClass::UnverifiedAttribution as i32,
        }
    }

    fn node_key() -> SigningKey {
        SigningKey::from_bytes(&[9; 32])
    }

    /// An envelope a node actually signed, claiming to be an enrolled node.
    fn signed_envelope(payload: &[u8], key: &SigningKey) -> v1::EventEnvelope {
        let mut wire = envelope(payload);
        wire.provenance = v1::ProvenanceClass::EnrolledNode as i32;
        wire.signing_key_id = public_key_fingerprint(&key.verifying_key().to_bytes());
        wire.signature = key.sign(&envelope_signing_bytes(&wire)).to_bytes().to_vec();
        wire
    }

    fn lifecycle_for(key: &SigningKey) -> SigningKeyLifecycle {
        let fingerprint = public_key_fingerprint(&key.verifying_key().to_bytes());
        SigningKeyLifecycle {
            record: SigningKeyRecord {
                signing_key_id: fingerprint.clone(),
                tenant_id: "acme".to_string(),
                repository_id: "repo".to_string(),
                node_id: "node-1".to_string(),
                public_key: key.verifying_key().to_bytes().to_vec(),
                public_key_fingerprint: fingerprint,
                activated_at: SystemTime::UNIX_EPOCH,
                expires_at: None,
            },
            retired_at: None,
            revoked_at: None,
        }
    }

    /// A resolver that judges against a real lifecycle, so tests exercise the
    /// same decision the database path does rather than a hand-made verdict.
    fn resolving(
        lifecycle: SigningKeyLifecycle,
    ) -> impl FnMut(OwnedEnvelopeBinding) -> std::future::Ready<Result<KeyResolution, SigningKeyError>>
    {
        move |binding: OwnedEnvelopeBinding| {
            let resolution = if binding.signing_key_id == lifecycle.record.signing_key_id {
                crate::signing_keys::judge(&lifecycle, &binding.as_binding())
            } else {
                KeyResolution::Unknown
            };
            ready(Ok(resolution))
        }
    }

    /// For tests that never present a key.
    fn no_key(
        _: OwnedEnvelopeBinding,
    ) -> std::future::Ready<Result<KeyResolution, SigningKeyError>> {
        ready(Ok(KeyResolution::Unknown))
    }

    fn flow_control() -> v1::FlowControl {
        v1::FlowControl {
            max_in_flight_batches: 3,
            max_batch_bytes: 4_096,
        }
    }

    #[tokio::test]
    async fn hello_is_acknowledged_without_advertising_unselected_capabilities() {
        let response = handle_frame(
            v1::NodeFrame {
                frame: Some(v1::node_frame::Frame::Hello(v1::Hello {
                    tenant_id: "acme".to_string(),
                    repository_id: "repo".to_string(),
                    producer_id: "node-1".to_string(),
                    last_accepted_position: 12,
                    capabilities: vec!["evidence-v1".to_string()],
                })),
            },
            flow_control(),
            |_| ready(Ok(AppendOutcome::Accepted { position: 1 })),
            no_key,
        )
        .await;

        assert_eq!(
            response,
            vec![
                v1::AckplaneFrame {
                    frame: Some(v1::ackplane_frame::Frame::HelloAccepted(
                        v1::HelloAccepted {
                            accepted_position: 12,
                            enabled_capabilities: Vec::new(),
                        }
                    )),
                },
                v1::AckplaneFrame {
                    frame: Some(v1::ackplane_frame::Frame::FlowControl(flow_control())),
                },
            ]
        );
    }

    #[tokio::test]
    async fn accepted_event_batch_returns_the_durable_receipt() {
        let response = handle_frame(
            v1::NodeFrame {
                frame: Some(v1::node_frame::Frame::EventBatch(v1::EventBatch {
                    events: vec![envelope(b"fact")],
                })),
            },
            flow_control(),
            |_| ready(Ok(AppendOutcome::Accepted { position: 19 })),
            no_key,
        )
        .await;

        assert_eq!(
            response,
            vec![v1::AckplaneFrame {
                frame: Some(v1::ackplane_frame::Frame::BatchReceipt(v1::BatchReceipt {
                    receipts: vec![v1::RecordReceipt {
                        record_id: "acme/repo/node-1@7".to_string(),
                        position: 19,
                        disposition: v1::ReceiptDisposition::Accepted as i32,
                    }],
                })),
            }]
        );
    }

    #[tokio::test]
    async fn malformed_event_batch_is_rejected_without_touching_the_ledger() {
        let mut invalid = envelope(b"fact");
        invalid.payload_digest = Sha256::digest(b"different").to_vec();
        let append_calls = Arc::new(AtomicUsize::new(0));
        let observed_calls = Arc::clone(&append_calls);

        let response = handle_frame(
            v1::NodeFrame {
                frame: Some(v1::node_frame::Frame::EventBatch(v1::EventBatch {
                    events: vec![invalid],
                })),
            },
            flow_control(),
            move |_| {
                observed_calls.fetch_add(1, Ordering::SeqCst);
                ready(Ok(AppendOutcome::Accepted { position: 1 }))
            },
            no_key,
        )
        .await;

        assert_eq!(append_calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            response,
            vec![v1::AckplaneFrame {
                frame: Some(v1::ackplane_frame::Frame::Rejection(v1::Rejection {
                    record_id: "acme/repo/node-1@7".to_string(),
                    reason: v1::RejectionReason::Malformed as i32,
                    retryable: false,
                    diagnostic: "payload_digest does not match the SHA-256 of payload".to_string(),
                })),
            }]
        );
    }

    // --- the trust boundary ------------------------------------------------

    /// The rejection a refused envelope produces, and whether the ledger was
    /// touched. Every test below asserts both: a forged record that reaches
    /// storage has already been trusted, so "was it refused" is only half the
    /// property.
    async fn outcome_of(
        wire: v1::EventEnvelope,
        resolve: impl FnMut(
            OwnedEnvelopeBinding,
        ) -> std::future::Ready<Result<KeyResolution, SigningKeyError>>,
    ) -> (Vec<v1::AckplaneFrame>, usize) {
        let appends = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&appends);
        let response = handle_frame(
            v1::NodeFrame {
                frame: Some(v1::node_frame::Frame::EventBatch(v1::EventBatch {
                    events: vec![wire],
                })),
            },
            flow_control(),
            move |_| {
                counted.fetch_add(1, Ordering::SeqCst);
                ready(Ok(AppendOutcome::Accepted { position: 1 }))
            },
            resolve,
        )
        .await;
        (response, appends.load(Ordering::SeqCst))
    }

    fn refusal_of(response: &[v1::AckplaneFrame]) -> &v1::Rejection {
        match response.first().and_then(|frame| frame.frame.as_ref()) {
            Some(v1::ackplane_frame::Frame::Rejection(rejection)) => rejection,
            other => panic!("expected a rejection, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_properly_signed_enrolled_record_is_accepted() {
        let key = node_key();
        let (response, appends) = outcome_of(
            signed_envelope(b"fact", &key),
            resolving(lifecycle_for(&key)),
        )
        .await;

        assert_eq!(appends, 1, "a valid signature must reach the ledger");
        assert!(matches!(
            response.first().and_then(|frame| frame.frame.as_ref()),
            Some(v1::ackplane_frame::Frame::BatchReceipt(_))
        ));
    }

    #[tokio::test]
    async fn an_altered_payload_is_refused_before_the_ledger() {
        // The signature covers payload_digest, so editing the payload and its
        // digest together still breaks it. This is the forgery the whole task
        // exists to stop.
        let key = node_key();
        let mut wire = signed_envelope(b"fact", &key);
        wire.payload = b"tampered".to_vec();
        wire.payload_digest = Sha256::digest(b"tampered").to_vec();

        let (response, appends) = outcome_of(wire, resolving(lifecycle_for(&key))).await;

        assert_eq!(appends, 0, "a forged record must never reach the ledger");
        assert_eq!(
            refusal_of(&response).reason,
            v1::RejectionReason::Unauthenticated as i32
        );
        assert!(!refusal_of(&response).retryable);
    }

    #[tokio::test]
    async fn a_signature_from_another_key_is_refused() {
        // Signed by an impostor but naming the enrolled key's id: the id says
        // one thing and the bytes prove another.
        let enrolled = node_key();
        let impostor = SigningKey::from_bytes(&[11; 32]);
        let mut wire = signed_envelope(b"fact", &enrolled);
        wire.signature = impostor
            .sign(&envelope_signing_bytes(&wire))
            .to_bytes()
            .to_vec();

        let (response, appends) = outcome_of(wire, resolving(lifecycle_for(&enrolled))).await;

        assert_eq!(appends, 0);
        assert_eq!(
            refusal_of(&response).diagnostic,
            "the signature does not verify under the enrolled key"
        );
    }

    #[tokio::test]
    async fn a_key_revoked_before_the_record_arrived_cannot_sign_it() {
        let key = node_key();
        let mut lifecycle = lifecycle_for(&key);
        lifecycle.revoked_at = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1));

        let (response, appends) =
            outcome_of(signed_envelope(b"fact", &key), resolving(lifecycle)).await;

        assert_eq!(appends, 0);
        assert_eq!(
            refusal_of(&response).reason,
            v1::RejectionReason::NodeRevoked as i32
        );
        assert!(!refusal_of(&response).retryable);
    }

    /// The half of ADR-0085 decision 8 a per-record check alone cannot give:
    /// a revoked node's connection must actually close, not merely keep
    /// refusing one record after another forever.
    #[tokio::test]
    async fn a_revoked_key_s_rejection_terminates_the_connection() {
        let key = node_key();
        let mut lifecycle = lifecycle_for(&key);
        lifecycle.revoked_at = Some(SystemTime::UNIX_EPOCH + Duration::from_secs(1));

        let (response, _) = outcome_of(signed_envelope(b"fact", &key), resolving(lifecycle)).await;

        assert!(must_terminate_after(&response));
    }

    /// Every OTHER rejection reason must leave the connection open: a
    /// malformed record, a bad signature, or a not-yet-active key are all
    /// things a sender can retry or correct on the same stream. Termination
    /// is specific to revocation, not a blanket response to any refusal.
    #[tokio::test]
    async fn an_ordinary_rejection_does_not_terminate_the_connection() {
        let mut invalid = envelope(b"fact");
        invalid.payload_digest = Sha256::digest(b"different").to_vec();

        let response = handle_frame(
            v1::NodeFrame {
                frame: Some(v1::node_frame::Frame::EventBatch(v1::EventBatch {
                    events: vec![invalid],
                })),
            },
            flow_control(),
            |_| ready(Ok(AppendOutcome::Accepted { position: 1 })),
            no_key,
        )
        .await;

        assert!(!must_terminate_after(&response));
    }

    #[test]
    fn must_terminate_after_is_true_only_for_a_node_revoked_rejection() {
        let revoked = vec![v1::AckplaneFrame {
            frame: Some(v1::ackplane_frame::Frame::Rejection(v1::Rejection {
                record_id: "acme/repo/node-1@7".to_string(),
                reason: v1::RejectionReason::NodeRevoked as i32,
                retryable: false,
                diagnostic: "revoked".to_string(),
            })),
        }];
        let unauthenticated = vec![v1::AckplaneFrame {
            frame: Some(v1::ackplane_frame::Frame::Rejection(v1::Rejection {
                record_id: "acme/repo/node-1@7".to_string(),
                reason: v1::RejectionReason::Unauthenticated as i32,
                retryable: false,
                diagnostic: "not in force".to_string(),
            })),
        }];
        let receipt = vec![v1::AckplaneFrame {
            frame: Some(v1::ackplane_frame::Frame::BatchReceipt(v1::BatchReceipt {
                receipts: vec![],
            })),
        }];

        assert!(must_terminate_after(&revoked));
        assert!(!must_terminate_after(&unauthenticated));
        assert!(!must_terminate_after(&receipt));
        assert!(!must_terminate_after(&[]));
    }

    #[tokio::test]
    async fn a_key_enrolled_to_another_node_is_unauthorized_not_unauthenticated() {
        // A real key presented for an identity it does not cover is a different
        // incident from an unknown key, and the reason code has to say so.
        let key = node_key();
        let mut lifecycle = lifecycle_for(&key);
        lifecycle.record.node_id = "node-2".to_string();

        let (response, appends) =
            outcome_of(signed_envelope(b"fact", &key), resolving(lifecycle)).await;

        assert_eq!(appends, 0);
        assert_eq!(
            refusal_of(&response).reason,
            v1::RejectionReason::Unauthorized as i32
        );
    }

    #[tokio::test]
    async fn a_declared_class_this_authority_cannot_prove_is_refused_not_downgraded() {
        // The substance of ADR-0084: a sender may not simply declare a stronger
        // trust class. Ackplane can substantiate enrolled_node and nothing else
        // yet, so authenticated_principal is refused rather than quietly stored
        // as something weaker the producer never claimed.
        let key = node_key();
        let mut wire = signed_envelope(b"fact", &key);
        wire.provenance = v1::ProvenanceClass::AuthenticatedPrincipal as i32;

        let (response, appends) = outcome_of(wire, resolving(lifecycle_for(&key))).await;

        assert_eq!(appends, 0, "an unprovable class must not be stored at all");
        assert_eq!(
            refusal_of(&response).diagnostic,
            "this authority cannot substantiate the declared provenance class; \
             declare enrolled_node and sign, or unverified_attribution"
        );
    }

    #[tokio::test]
    async fn an_enrolled_claim_without_a_signature_is_refused() {
        let key = node_key();
        let mut wire = signed_envelope(b"fact", &key);
        wire.signature = Vec::new();

        let (response, appends) = outcome_of(wire, resolving(lifecycle_for(&key))).await;

        assert_eq!(appends, 0);
        assert_eq!(
            refusal_of(&response).diagnostic,
            "the declared provenance class requires a signature and none was sent"
        );
    }

    #[tokio::test]
    async fn an_unavailable_key_registry_is_retryable_not_a_rejection_of_the_sender() {
        // A registry that cannot answer is not a node that cannot authenticate.
        // Refusing this as unauthenticated would be non-retryable and would
        // permanently reject a record whose sender is very likely legitimate.
        let key = node_key();
        let (response, appends) = outcome_of(signed_envelope(b"fact", &key), |_| {
            ready(Err(SigningKeyError::Database(
                tokio_postgres::Error::__private_api_timeout(),
            )))
        })
        .await;

        assert_eq!(appends, 0);
        assert!(refusal_of(&response).retryable);
        assert_eq!(
            refusal_of(&response).reason,
            v1::RejectionReason::Unavailable as i32
        );
    }

    #[tokio::test]
    async fn heartbeat_elicits_no_reply() {
        let response = handle_frame(
            v1::NodeFrame {
                frame: Some(v1::node_frame::Frame::Heartbeat(v1::Heartbeat {
                    last_accepted_position: 12,
                })),
            },
            flow_control(),
            |_| ready(Ok(AppendOutcome::Accepted { position: 1 })),
            no_key,
        )
        .await;

        assert!(response.is_empty());
    }
}

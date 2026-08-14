//! gRPC transport for the node-to-ledger synchronization stream (ADR-0083).

use std::{future::Future, pin::Pin, sync::Arc};

use ackplane_protocol::v1;
use tokio::sync::{mpsc, Mutex};
use tokio_stream::{wrappers::ReceiverStream, Stream};
use tonic::{Request, Response, Status};

use crate::{
    ledger::{AppendError, AppendOutcome, EventEnvelope, LedgerStore},
    sync,
};

/// Process one node frame with the supplied durable append operation.
///
/// The transport supplies the real ledger append; tests supply a deterministic
/// operation so protocol decisions remain testable without PostgreSQL.
pub async fn handle_frame<F, Fut>(
    frame: v1::NodeFrame,
    flow_control: v1::FlowControl,
    mut append: F,
) -> Vec<v1::AckplaneFrame>
where
    F: FnMut(EventEnvelope) -> Fut,
    Fut: Future<Output = Result<AppendOutcome, AppendError>>,
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
                        let responses = handle_frame(frame, flow_control, |envelope| {
                            let ledger = Arc::clone(&ledger);
                            async move { ledger.lock().await.append(&envelope).await }
                        })
                        .await;
                        for frame in responses {
                            if sender.send(Ok(frame)).await.is_err() {
                                break 'stream;
                            }
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
    };

    use sha2::{Digest, Sha256};

    use super::*;

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
            provenance: v1::ProvenanceClass::EnrolledNode as i32,
        }
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
        )
        .await;

        assert!(response.is_empty());
    }
}

//! A genuine, authenticated `Synchronize` connection to a running Ackplane
//! deployment (ADR-0116 decision 2: "The supervisor makes outbound
//! authenticated connections... completes the enrolled-key challenge").
//!
//! This proves the same Hello -> ConnectionChallenge -> ChallengeResponse ->
//! HelloAccepted -> FlowControl handshake
//! `crates/ackplane-server/src/service/handshake.rs` already implements and
//! tests server-side, as one reusable client type instead of reimplemented
//! inline per integration -- the only prior example of this handshake from
//! the client side, `ackplane-client/examples/enroll_and_sync.rs`, built it
//! inline and imported `ackplane_server::enrollment` directly, a dependency a
//! reusable client library must not carry (this crate's own module doc: "must
//! not smuggle a plane dependency back"; the same boundary applies to a
//! server-crate dependency for a byte-exact signing contract). That shared
//! contract now lives in `ackplane_protocol::connection_challenge_auth`.
//!
//! Deliberately narrow: [`NodeSyncConnection::open`] returns only after the
//! handshake completes, and exposes nothing beyond the raw frame
//! sender/receiver plus what the server accepted. It sends or receives no
//! other frame itself -- registration, heartbeats, directive delivery, and
//! reconnect reconciliation are later ADR-0116 slices built on top of the
//! connection this returns.

use std::collections::VecDeque;

use ackplane_protocol::connection_challenge_auth::{
    connection_challenge_bytes, ConnectionChallengeBinding,
};
use ackplane_protocol::v1::{self, node_sync_service_client::NodeSyncServiceClient};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{transport::Channel, Streaming};

use crate::auth::ClaimSigner;
use crate::{ClientError, CONNECT_TIMEOUT};

/// How many outbound frames may be queued locally before `send` blocks. Small
/// and fixed: this connection carries control frames, not a bulk transfer
/// channel, so an unbounded queue would only hide a stalled server behind
/// unbounded memory growth instead of surfacing backpressure to the caller.
const OUTBOUND_CHANNEL_CAPACITY: usize = 16;

/// A live, authenticated `Synchronize` connection. A value of this type
/// exists only once the server has accepted `HelloAccepted` and sent its
/// initial `FlowControl` -- nothing reachable before that point in the
/// handshake is ever exposed as a value a caller could try to use.
pub struct NodeSyncConnection {
    tx: mpsc::Sender<v1::NodeFrame>,
    rx: Streaming<v1::AckplaneFrame>,
    accepted_position: u64,
    enabled_capabilities: Vec<String>,
    flow_control: v1::FlowControl,
    /// Directives that arrived while awaiting a receipt.
    ///
    /// The server delivers pending directives immediately after the receipt
    /// for the session frame that addressed them, so a caller waiting on that
    /// receipt is the very caller that sees them first. Buffering keeps
    /// `exchange_supervisor_frame`'s promise that a directive is never
    /// silently dropped, without forcing every caller to become a directive
    /// handler.
    directives: VecDeque<v1::AgentDirective>,
}

impl NodeSyncConnection {
    /// Opens a `Synchronize` stream to `endpoint`, sends `Hello`, and
    /// completes the enrolled-key challenge using `signer` (ADR-0116 decision
    /// 8: signing goes through the same [`ClaimSigner`] abstraction the claim
    /// flow already uses -- this method never touches a raw private key
    /// itself).
    ///
    /// `capabilities` names the transport-level features this connection is
    /// declaring (ADR-0116 decision 4 keeps deciding what any of that
    /// authorizes downstream of this handshake; a node's requested
    /// capabilities here are not proof the server enables them, mirrored by
    /// the server's own `HelloAccepted.enabled_capabilities` this method
    /// returns via [`Self::enabled_capabilities`]).
    pub async fn open(
        endpoint: &str,
        signer: &dyn ClaimSigner,
        tenant_id: &str,
        repository_id: &str,
        capabilities: Vec<String>,
        last_accepted_position: u64,
    ) -> Result<Self, ClientError> {
        let channel = Channel::from_shared(endpoint.to_string())
            .map_err(|_| ClientError::InvalidEndpoint(endpoint.to_string()))?
            .connect_timeout(CONNECT_TIMEOUT)
            .connect()
            .await?;
        let mut client = NodeSyncServiceClient::new(channel);

        let (tx, request_rx) = mpsc::channel::<v1::NodeFrame>(OUTBOUND_CHANNEL_CAPACITY);
        send(
            &tx,
            v1::NodeFrame {
                frame: Some(v1::node_frame::Frame::Hello(v1::Hello {
                    tenant_id: tenant_id.to_string(),
                    repository_id: repository_id.to_string(),
                    producer_id: signer.node_id().to_string(),
                    last_accepted_position,
                    capabilities,
                    signing_key_id: signer.signing_key_id().to_string(),
                })),
            },
        )
        .await?;

        let response = client
            .synchronize(tonic::Request::new(ReceiverStream::new(request_rx)))
            .await?;
        let mut rx = response.into_inner();

        let nonce = match next_frame(&mut rx, "ConnectionChallenge").await? {
            v1::ackplane_frame::Frame::ConnectionChallenge(challenge) => challenge.nonce,
            v1::ackplane_frame::Frame::Rejection(rejection) => return Err(refused(rejection)),
            other => {
                return Err(ClientError::UnexpectedHandshakeFrame {
                    expected: "ConnectionChallenge",
                    got: frame_name(&other),
                })
            }
        };

        let signature = signer.sign(&connection_challenge_bytes(&ConnectionChallengeBinding {
            nonce: &nonce,
            tenant_id,
            repository_id,
            producer_id: signer.node_id(),
            signing_key_id: signer.signing_key_id(),
        }));
        send(
            &tx,
            v1::NodeFrame {
                frame: Some(v1::node_frame::Frame::ChallengeResponse(
                    v1::ChallengeResponse { signature },
                )),
            },
        )
        .await?;

        let accepted = match next_frame(&mut rx, "HelloAccepted").await? {
            v1::ackplane_frame::Frame::HelloAccepted(accepted) => accepted,
            v1::ackplane_frame::Frame::Rejection(rejection) => return Err(refused(rejection)),
            other => {
                return Err(ClientError::UnexpectedHandshakeFrame {
                    expected: "HelloAccepted",
                    got: frame_name(&other),
                })
            }
        };
        let flow_control = match next_frame(&mut rx, "FlowControl").await? {
            v1::ackplane_frame::Frame::FlowControl(flow_control) => flow_control,
            v1::ackplane_frame::Frame::Rejection(rejection) => return Err(refused(rejection)),
            other => {
                return Err(ClientError::UnexpectedHandshakeFrame {
                    expected: "FlowControl",
                    got: frame_name(&other),
                })
            }
        };

        Ok(Self {
            tx,
            rx,
            accepted_position: accepted.accepted_position,
            enabled_capabilities: accepted.enabled_capabilities,
            flow_control,
            directives: VecDeque::new(),
        })
    }

    /// The position the server had already durably accepted before this
    /// connection opened. A future reconnect slice compares its own durable
    /// local state against this rather than assuming a fresh connection means
    /// a fresh history.
    pub fn accepted_position(&self) -> u64 {
        self.accepted_position
    }

    /// The capabilities the server actually enabled for this connection --
    /// never assume every requested capability was granted.
    pub fn enabled_capabilities(&self) -> &[String] {
        &self.enabled_capabilities
    }

    /// The flow-control terms the server announced immediately after
    /// authentication.
    pub fn flow_control(&self) -> &v1::FlowControl {
        &self.flow_control
    }

    /// Send one frame on this authenticated connection.
    pub async fn send(&self, frame: v1::NodeFrame) -> Result<(), ClientError> {
        send(&self.tx, frame).await
    }

    /// Receive the next frame from the server, or `Ok(None)` when the server
    /// closed the stream cleanly.
    pub async fn recv(&mut self) -> Result<Option<v1::AckplaneFrame>, ClientError> {
        Ok(self.rx.message().await?)
    }

    /// Send one frame and wait for the receipt it earns.
    ///
    /// The handshake already turns a refusal into a typed error. Without the
    /// same treatment here the authenticated phase is harder to get right than
    /// the handshake was: a refusal arrives as an ordinary frame, so a caller
    /// that only looked for its receipt would wait for one that is never coming
    /// and report a timeout instead of the reason the server gave it.
    ///
    /// Flow control and notices are server housekeeping and are stepped over.
    /// A directive is buffered for [`Self::next_directive`] rather than
    /// dropped or treated as an error: the server delivers pending directives
    /// right behind the receipt for the session frame that addressed them, so
    /// they legitimately arrive here. Anything else is returned as an error
    /// naming it, so a frame this caller does not handle is never silently
    /// dropped.
    async fn exchange<T>(
        &mut self,
        frame: v1::NodeFrame,
        expected: &'static str,
        mut receipt: impl FnMut(v1::ackplane_frame::Frame) -> Result<T, v1::ackplane_frame::Frame>,
    ) -> Result<T, ClientError> {
        self.send(frame).await?;
        loop {
            let Some(envelope) = self.recv().await? else {
                return Err(ClientError::UnexpectedFrame {
                    expected,
                    got: "a closed stream",
                });
            };
            let Some(frame) = envelope.frame else {
                continue;
            };
            match frame {
                v1::ackplane_frame::Frame::Rejection(rejection) => {
                    return Err(frame_refused(rejection))
                }
                v1::ackplane_frame::Frame::AgentDirective(directive) => {
                    self.directives.push_back(*directive);
                    continue;
                }
                v1::ackplane_frame::Frame::FlowControl(_)
                | v1::ackplane_frame::Frame::Notice(_) => continue,
                other => match receipt(other) {
                    Ok(receipt) => return Ok(receipt),
                    Err(unexpected) => {
                        return Err(ClientError::UnexpectedFrame {
                            expected,
                            got: frame_name(&unexpected),
                        })
                    }
                },
            }
        }
    }

    /// Send one supervisor frame and wait for the receipt it earns.
    pub async fn exchange_supervisor_frame(
        &mut self,
        frame: v1::NodeFrame,
    ) -> Result<v1::SupervisorFrameReceipt, ClientError> {
        self.exchange(frame, "SupervisorFrameReceipt", |frame| match frame {
            v1::ackplane_frame::Frame::SupervisorFrameReceipt(receipt) => Ok(receipt),
            other => Err(other),
        })
        .await
    }

    /// Publish one event batch and wait for its durable receipt.
    pub async fn exchange_event_batch(
        &mut self,
        frame: v1::NodeFrame,
    ) -> Result<v1::BatchReceipt, ClientError> {
        self.exchange(frame, "BatchReceipt", |frame| match frame {
            v1::ackplane_frame::Frame::BatchReceipt(receipt) => Ok(receipt),
            other => Err(other),
        })
        .await
    }

    /// The next directive this connection has received, if any is already in
    /// hand.
    ///
    /// Non-blocking, and deterministic rather than racy: the server sends a
    /// session's pending directives *ahead* of the receipt for the frame that
    /// addressed them, so the receipt is a delivery barrier. A caller holding
    /// that receipt has necessarily already read every directive delivered
    /// with it, and draining here cannot miss one that is still in flight.
    pub fn next_directive(&mut self) -> Option<v1::AgentDirective> {
        self.directives.pop_front()
    }

    /// Return the durable receipt for a directive this supervisor processed.
    ///
    /// The server answers a `DirectiveReceipt` with an ordinary supervisor
    /// frame receipt, so this is the same round trip as any other supervisor
    /// frame -- including its refusal handling.
    pub async fn submit_directive_receipt(
        &mut self,
        receipt: v1::DirectiveReceipt,
    ) -> Result<v1::SupervisorFrameReceipt, ClientError> {
        self.exchange_supervisor_frame(v1::NodeFrame {
            frame: Some(v1::node_frame::Frame::DirectiveReceipt(receipt)),
        })
        .await
    }
}

async fn send(tx: &mpsc::Sender<v1::NodeFrame>, frame: v1::NodeFrame) -> Result<(), ClientError> {
    tx.send(frame)
        .await
        .map_err(|_| ClientError::HandshakeStreamClosed)
}

async fn next_frame(
    rx: &mut Streaming<v1::AckplaneFrame>,
    expected: &'static str,
) -> Result<v1::ackplane_frame::Frame, ClientError> {
    let frame = rx
        .message()
        .await?
        .ok_or(ClientError::HandshakeStreamClosed)?;
    frame.frame.ok_or(ClientError::UnexpectedHandshakeFrame {
        expected,
        got: "an empty frame",
    })
}

fn refused(rejection: v1::Rejection) -> ClientError {
    let reason =
        v1::RejectionReason::try_from(rejection.reason).unwrap_or(v1::RejectionReason::Unspecified);
    ClientError::ConnectionRefused {
        reason,
        retryable: rejection.retryable,
        diagnostic: rejection.diagnostic,
    }
}

fn frame_refused(rejection: v1::Rejection) -> ClientError {
    let reason =
        v1::RejectionReason::try_from(rejection.reason).unwrap_or(v1::RejectionReason::Unspecified);
    ClientError::FrameRefused {
        reason,
        retryable: rejection.retryable,
        diagnostic: rejection.diagnostic,
    }
}

fn frame_name(frame: &v1::ackplane_frame::Frame) -> &'static str {
    match frame {
        v1::ackplane_frame::Frame::HelloAccepted(_) => "HelloAccepted",
        v1::ackplane_frame::Frame::BatchReceipt(_) => "BatchReceipt",
        v1::ackplane_frame::Frame::Rejection(_) => "Rejection",
        v1::ackplane_frame::Frame::FlowControl(_) => "FlowControl",
        v1::ackplane_frame::Frame::Notice(_) => "Notice",
        v1::ackplane_frame::Frame::ConnectionChallenge(_) => "ConnectionChallenge",
        v1::ackplane_frame::Frame::AgentDirective(_) => "AgentDirective",
        v1::ackplane_frame::Frame::SupervisorFrameReceipt(_) => "SupervisorFrameReceipt",
        v1::ackplane_frame::Frame::WorkTaskReceipt(_) => "WorkTaskReceipt",
    }
}

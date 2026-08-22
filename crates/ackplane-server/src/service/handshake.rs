//! ADR-0098 decision 1's mandatory connection handshake: `Hello` and its
//! `ChallengeResponse`, and the connection-level refusal both share.

use std::time::SystemTime;

use ackplane_protocol::v1;

use super::{ConnectionState, OwnedEnvelopeBinding};
use crate::signing_keys::{KeyResolution, SigningKeyError};

/// Refuse the whole connection (ADR-0098 decision 1's handshake gate), distinct
/// from `unauthenticated`: that rejects one record and lets the sender retry
/// on the same stream, this ends the stream because nothing sent on it can be
/// trusted without a completed handshake.
pub(super) fn connection_refused(diagnostic: &str) -> v1::AckplaneFrame {
    v1::AckplaneFrame {
        frame: Some(v1::ackplane_frame::Frame::Rejection(v1::Rejection {
            record_id: String::new(),
            reason: v1::RejectionReason::Unauthenticated as i32,
            retryable: false,
            diagnostic: diagnostic.to_string(),
        })),
    }
}

/// `Hello`'s half of the handshake: issue a fresh, single-use, domain-separated
/// nonce and wait for the node to sign it with the key it just named. No
/// database access here - a nonce challenge needs no lookup, only the answer
/// to it does.
pub(super) fn handle_hello(hello: v1::Hello) -> (ConnectionState, Vec<v1::AckplaneFrame>) {
    if hello.signing_key_id.is_empty() {
        return (
            ConnectionState::Rejected,
            vec![connection_refused(
                "Hello must name the signing_key_id this connection will authenticate with",
            )],
        );
    }
    let mut nonce = [0_u8; 32];
    if getrandom::getrandom(&mut nonce).is_err() {
        return (
            ConnectionState::Rejected,
            vec![connection_refused(
                "could not generate a connection challenge nonce",
            )],
        );
    }
    let nonce = nonce.to_vec();
    let state = ConnectionState::AwaitingChallengeResponse {
        nonce: nonce.clone(),
        tenant_id: hello.tenant_id,
        repository_id: hello.repository_id,
        producer_id: hello.producer_id,
        signing_key_id: hello.signing_key_id,
        last_accepted_position: hello.last_accepted_position,
    };
    let frames = vec![v1::AckplaneFrame {
        frame: Some(v1::ackplane_frame::Frame::ConnectionChallenge(
            v1::ConnectionChallenge { nonce },
        )),
    }];
    (state, frames)
}

/// `ChallengeResponse`'s half of the handshake: resolve the named key as of
/// now (a key revoked or expired after this instant must not retroactively
/// invalidate a stream it was never used to open) and verify the signature
/// over this connection's own nonce.
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_challenge_response<R, RFut>(
    response: v1::ChallengeResponse,
    nonce: Vec<u8>,
    tenant_id: String,
    repository_id: String,
    producer_id: String,
    signing_key_id: String,
    last_accepted_position: u64,
    flow_control: v1::FlowControl,
    resolve_key: &mut R,
) -> (ConnectionState, Vec<v1::AckplaneFrame>)
where
    R: FnMut(OwnedEnvelopeBinding) -> RFut,
    RFut: std::future::Future<Output = Result<KeyResolution, SigningKeyError>>,
{
    let resolution = match resolve_key(OwnedEnvelopeBinding {
        signing_key_id: signing_key_id.clone(),
        tenant_id: tenant_id.clone(),
        repository_id: repository_id.clone(),
        producer_id: producer_id.clone(),
        accepted_at: SystemTime::now(),
    })
    .await
    {
        Ok(resolution) => resolution,
        // A key store that cannot answer is not a node that cannot
        // authenticate; refusing the whole connection non-retryably would be
        // wrong for a very likely legitimate node.
        Err(error) => {
            tracing::error!(%error, "signing key lookup failed during connection authentication");
            return (
                ConnectionState::Rejected,
                vec![v1::AckplaneFrame {
                    frame: Some(v1::ackplane_frame::Frame::Rejection(v1::Rejection {
                        record_id: String::new(),
                        reason: v1::RejectionReason::Unavailable as i32,
                        retryable: true,
                        diagnostic: "the key registry was unavailable; retry the connection"
                            .to_string(),
                    })),
                }],
            );
        }
    };

    let record = match resolution {
        KeyResolution::Resolved(record) => record,
        KeyResolution::Revoked => {
            return (
                ConnectionState::Rejected,
                vec![v1::AckplaneFrame {
                    frame: Some(v1::ackplane_frame::Frame::Rejection(v1::Rejection {
                        record_id: String::new(),
                        reason: v1::RejectionReason::NodeRevoked as i32,
                        retryable: false,
                        diagnostic: "this node's signing key has been revoked".to_string(),
                    })),
                }],
            );
        }
        KeyResolution::Unknown
        | KeyResolution::BindingMismatch
        | KeyResolution::NotYetActive
        | KeyResolution::Expired
        | KeyResolution::Retired => {
            return (
                ConnectionState::Rejected,
                vec![connection_refused(
                    "the named signing_key_id is not an active, enrolled key for this \
                     tenant/repository/node",
                )],
            );
        }
    };

    let verified = crate::enrollment::verify_connection_challenge(
        &record.public_key,
        &response.signature,
        crate::enrollment::ConnectionChallengeBinding {
            nonce: &nonce,
            tenant_id: &tenant_id,
            repository_id: &repository_id,
            producer_id: &producer_id,
            signing_key_id: &signing_key_id,
        },
    );
    if !verified {
        return (
            ConnectionState::Rejected,
            vec![connection_refused(
                "the connection challenge response does not verify under the named key",
            )],
        );
    }

    let state = ConnectionState::Authenticated {
        tenant_id,
        repository_id,
        producer_id,
    };
    let frames = vec![
        v1::AckplaneFrame {
            frame: Some(v1::ackplane_frame::Frame::HelloAccepted(
                v1::HelloAccepted {
                    accepted_position: last_accepted_position,
                    // A node's requested capabilities are not proof that the
                    // server enables them. Advertise only capabilities this
                    // transport has explicitly selected.
                    enabled_capabilities: Vec::new(),
                },
            )),
        },
        v1::AckplaneFrame {
            frame: Some(v1::ackplane_frame::Frame::FlowControl(flow_control)),
        },
    ];
    (state, frames)
}

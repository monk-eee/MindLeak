//! The exact bytes an enrolled node signs to prove it holds the key it named
//! in `Hello`, over one `Synchronize` connection's nonce (ADR-0098 decision 1,
//! ADR-0116 decision 2). Shared by the server that issues and verifies the
//! challenge (`ackplane-server::enrollment`) and any client that opens the
//! connection (`ackplane-client::NodeSyncClient`), so the two sides can never
//! drift into incompatible byte layouts for the same signed fields -- moved
//! here from `ackplane-server::enrollment` once a second, genuinely separate
//! crate needed to produce these bytes rather than only verify them.

use crate::signing_bytes::push_field;

/// Its own domain, distinct from activation and envelope signing (ADR-0098
/// decision 1): a signature over one of those must never verify as a
/// connection challenge response, or a replayed activation/envelope signature
/// could open a live stream it was never meant to authenticate.
pub const CONNECTION_DOMAIN: &[u8] = b"mindleak.ackplane.v1.node_sync.connection\0";

/// The immutable values a `Synchronize` connection's challenge binds together.
pub struct ConnectionChallengeBinding<'a> {
    pub nonce: &'a [u8],
    pub tenant_id: &'a str,
    pub repository_id: &'a str,
    pub producer_id: &'a str,
    pub signing_key_id: &'a str,
}

/// Encode the exact domain-separated bytes a node signs to prove it holds the
/// key it named in `Hello`, over this connection's nonce. Every field is
/// length-delimited (`signing_bytes::push_field`) so no field can be
/// reinterpreted as part of an adjacent one.
pub fn connection_challenge_bytes(binding: &ConnectionChallengeBinding<'_>) -> Vec<u8> {
    let fields: [&[u8]; 5] = [
        binding.nonce,
        binding.tenant_id.as_bytes(),
        binding.repository_id.as_bytes(),
        binding.producer_id.as_bytes(),
        binding.signing_key_id.as_bytes(),
    ];
    let mut bytes = Vec::with_capacity(
        CONNECTION_DOMAIN.len() + fields.iter().map(|field| 4 + field.len()).sum::<usize>(),
    );
    bytes.extend_from_slice(CONNECTION_DOMAIN);
    for field in fields {
        push_field(&mut bytes, field);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_challenge_encoding_binds_each_field_unambiguously() {
        let encoded = connection_challenge_bytes(&ConnectionChallengeBinding {
            nonce: &[7, 8],
            tenant_id: "tenant",
            repository_id: "repository",
            producer_id: "node-1",
            signing_key_id: "key-1",
        });

        assert_eq!(
            encoded,
            [
                b"mindleak.ackplane.v1.node_sync.connection\0".as_slice(),
                &[0, 0, 0, 2, 7, 8],
                &[0, 0, 0, 6],
                b"tenant",
                &[0, 0, 0, 10],
                b"repository",
                &[0, 0, 0, 6],
                b"node-1",
                &[0, 0, 0, 5],
                b"key-1",
            ]
            .concat()
        );
    }
}

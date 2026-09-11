//! Shared public-key fingerprints and activation signing bytes for node enrollment.

use sha2::{Digest, Sha256};

use crate::signing_bytes::push_field;

const ACTIVATION_DOMAIN: &[u8] = b"mindleak.ackplane.v1.enrollment.activation\0";

pub fn public_key_fingerprint(public_key: &[u8]) -> String {
    let digest = Sha256::digest(public_key);
    let encoded = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("ed25519:{encoded}")
}

pub fn activation_challenge_bytes(
    nonce: &[u8],
    request_id: &str,
    tenant_id: &str,
    repository_id: &str,
    node_id: &str,
    public_key_fingerprint: &str,
) -> Vec<u8> {
    let fields = [
        nonce,
        request_id.as_bytes(),
        tenant_id.as_bytes(),
        repository_id.as_bytes(),
        node_id.as_bytes(),
        public_key_fingerprint.as_bytes(),
    ];
    let mut bytes = Vec::with_capacity(
        ACTIVATION_DOMAIN.len() + fields.iter().map(|field| 4 + field.len()).sum::<usize>(),
    );
    bytes.extend_from_slice(ACTIVATION_DOMAIN);
    for field in fields {
        push_field(&mut bytes, field);
    }
    bytes
}

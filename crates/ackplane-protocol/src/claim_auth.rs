//! The exact bytes a repository node signs to authenticate a claim request
//! (ADR-0096 clause 4), shared by the server that verifies them
//! (`ackplane-server::claim_signature`) and the client that produces them
//! (`ackplane-client`). Kept at this shared layer so the two sides can never
//! drift into incompatible serializations of the same signed fields.

use crate::v1;

/// Domain separation for claim-request signatures.
pub const CLAIM_DOMAIN: &[u8] = b"mindleak.ackplane.v1.claim\0";

/// Binds the authentication to this specific claim's identity -- tenant,
/// repository, task, and the owner it is requesting on behalf of -- so a
/// signature valid for one claim can never verify against another, even from
/// the same key. Every field is length-delimited, following
/// `envelope_signature::envelope_signing_bytes`.
pub fn claim_signing_bytes(
    tenant_id: &str,
    repository_id: &str,
    task_id: &str,
    owner_id: &str,
    authentication: &v1::ClaimAuthentication,
) -> Vec<u8> {
    let fields: [&[u8]; 8] = [
        authentication.signing_key_id.as_bytes(),
        authentication.node_id.as_bytes(),
        authentication.signed_at.as_bytes(),
        &authentication.nonce,
        tenant_id.as_bytes(),
        repository_id.as_bytes(),
        task_id.as_bytes(),
        owner_id.as_bytes(),
    ];

    let mut bytes = Vec::with_capacity(
        CLAIM_DOMAIN.len() + fields.iter().map(|field| 4 + field.len()).sum::<usize>(),
    );
    bytes.extend_from_slice(CLAIM_DOMAIN);
    for field in fields {
        bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
        bytes.extend_from_slice(field);
    }
    bytes
}

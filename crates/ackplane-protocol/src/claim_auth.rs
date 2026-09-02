//! The exact bytes a repository node signs to authenticate a claim request
//! (ADR-0096 clause 4, ADR-0100 decision 10), shared by the server that
//! verifies them (`ackplane-server::claim_signature`) and the client that
//! produces them (`ackplane-client`). Kept at this shared layer so the two
//! sides can never drift into incompatible serializations of the same
//! signed fields.

use crate::signing_bytes::{push_field, push_list};
use crate::v1;

/// Domain separation for claim-request signatures.
pub const CLAIM_DOMAIN: &[u8] = b"mindleak.ackplane.v1.claim\0";

/// Which claim RPC an authentication authorizes, and that operation's own
/// fields.
///
/// Identity alone (tenant/repository/task/owner) proves *who* is asking, not
/// *what* they asked for: a signature covering only identity verifies
/// identically for `RenewClaim` with a 60-second lease and `RecoverClaim`
/// naming a different `expected_owner`, or for the same operation with a
/// different `branch`/`scope`/`reason`. Binding the operation tag and every
/// operation-specific field closes that gap (ADR-0100 decision 10).
#[derive(Debug, Clone, Copy)]
pub enum ClaimOperation<'a> {
    Delegate {
        branch: &'a str,
        lease_seconds: u64,
        paths: &'a [String],
        symbols: &'a [String],
    },
    Renew {
        lease_seconds: u64,
    },
    /// No operation-specific fields beyond identity: releasing carries
    /// nothing to bind.
    Release,
    Recover {
        expected_owner: &'a str,
        branch: &'a str,
        lease_seconds: u64,
        paths: &'a [String],
        symbols: &'a [String],
        reason: &'a str,
    },
    /// No operation-specific fields beyond identity: parking carries no free
    /// text (the question itself is local-only, ADR-0020's task_qa thread).
    Park,
    Answer {
        lease_seconds: u64,
    },
}

impl ClaimOperation<'_> {
    /// The distinct tag every variant signs, so a signature for one RPC can
    /// never verify for another even when every other field happens to
    /// coincide (e.g. `Release` and a zero-field `Renew`).
    fn tag(&self) -> &'static str {
        match self {
            Self::Delegate { .. } => "delegate",
            Self::Renew { .. } => "renew",
            Self::Release => "release",
            Self::Recover { .. } => "recover",
            Self::Park => "park",
            Self::Answer { .. } => "answer",
        }
    }

    fn push_fields(&self, bytes: &mut Vec<u8>) {
        push_field(bytes, self.tag().as_bytes());
        match self {
            Self::Delegate {
                branch,
                lease_seconds,
                paths,
                symbols,
            } => {
                push_field(bytes, branch.as_bytes());
                push_field(bytes, &lease_seconds.to_be_bytes());
                push_list(bytes, paths);
                push_list(bytes, symbols);
            }
            Self::Renew { lease_seconds } => {
                push_field(bytes, &lease_seconds.to_be_bytes());
            }
            Self::Release => {}
            Self::Recover {
                expected_owner,
                branch,
                lease_seconds,
                paths,
                symbols,
                reason,
            } => {
                push_field(bytes, expected_owner.as_bytes());
                push_field(bytes, branch.as_bytes());
                push_field(bytes, &lease_seconds.to_be_bytes());
                push_list(bytes, paths);
                push_list(bytes, symbols);
                push_field(bytes, reason.as_bytes());
            }
            Self::Park => {}
            Self::Answer { lease_seconds } => {
                push_field(bytes, &lease_seconds.to_be_bytes());
            }
        }
    }
}

/// Binds the authentication to this specific claim's identity -- tenant,
/// repository, task, and the owner it is requesting on behalf of -- and to
/// the exact operation and fields being authorized. Every field is
/// length-delimited, following `envelope_signature::envelope_signing_bytes`.
pub fn claim_signing_bytes(
    tenant_id: &str,
    repository_id: &str,
    task_id: &str,
    owner_id: &str,
    operation: &ClaimOperation,
    authentication: &v1::ClaimAuthentication,
) -> Vec<u8> {
    let identity_fields: [&[u8]; 8] = [
        authentication.signing_key_id.as_bytes(),
        authentication.node_id.as_bytes(),
        authentication.signed_at.as_bytes(),
        &authentication.nonce,
        tenant_id.as_bytes(),
        repository_id.as_bytes(),
        task_id.as_bytes(),
        owner_id.as_bytes(),
    ];

    let mut bytes = Vec::with_capacity(CLAIM_DOMAIN.len() + 64);
    bytes.extend_from_slice(CLAIM_DOMAIN);
    for field in identity_fields {
        push_field(&mut bytes, field);
    }
    operation.push_fields(&mut bytes);
    bytes
}

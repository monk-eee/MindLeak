//! Producing a `ClaimAuthentication` a repository node can send with a claim
//! request (ADR-0096 clause 4).
//!
//! Signing itself is abstracted behind [`ClaimSigner`] rather than fixed to
//! one key-sourcing mechanism: ADR-0085 decision 2 wants the private key
//! non-exportable through an OS credential facility where one exists. No such
//! integration exists in this workspace yet, so [`SeedSigner`] is an interim,
//! explicit-configuration implementation (a raw seed, sourced today from an
//! environment variable by the caller) -- the same posture already accepted
//! here for `MINDLEAK_LLM_API_KEY`. See
//! `gaps.d/the-node-signing-key-has-no-credential-facility-yet.md`.

use ed25519_dalek::{Signer, SigningKey};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub use ackplane_protocol::v1::ClaimAuthentication;

/// A repository node's capability to prove its identity for a claim request.
/// Deliberately agnostic to how the key is held -- only that it can sign.
pub trait ClaimSigner {
    fn signing_key_id(&self) -> &str;
    fn node_id(&self) -> &str;
    fn sign(&self, bytes: &[u8]) -> Vec<u8>;
}

/// An Ed25519 [`ClaimSigner`] built directly from a 32-byte seed.
pub struct SeedSigner {
    signing_key_id: String,
    node_id: String,
    key: SigningKey,
}

impl SeedSigner {
    pub fn new(
        signing_key_id: impl Into<String>,
        node_id: impl Into<String>,
        seed: &[u8; 32],
    ) -> Self {
        Self {
            signing_key_id: signing_key_id.into(),
            node_id: node_id.into(),
            key: SigningKey::from_bytes(seed),
        }
    }
}

impl ClaimSigner for SeedSigner {
    fn signing_key_id(&self) -> &str {
        &self.signing_key_id
    }

    fn node_id(&self) -> &str {
        &self.node_id
    }

    fn sign(&self, bytes: &[u8]) -> Vec<u8> {
        self.key.sign(bytes).to_bytes().to_vec()
    }
}

/// Build and sign a `ClaimAuthentication` for one claim request.
///
/// The nonce is fresh random bytes per call (`getrandom`, the same source
/// `ackplane-server`'s enrolment challenges use) -- not itself a freshness
/// guarantee the server enforces yet
/// (`gaps.d/claim-authentication-can-be-replayed-across-operations.md`
/// remains the tracked follow-up), but there is no reason to sign a
/// predictable value where an unpredictable one is this cheap.
pub fn authenticate(
    signer: &dyn ClaimSigner,
    tenant_id: &str,
    repository_id: &str,
    task_id: &str,
    owner_id: &str,
) -> ClaimAuthentication {
    let mut nonce = [0u8; 16];
    getrandom::getrandom(&mut nonce).expect("the OS random source should be available");
    let mut authentication = ClaimAuthentication {
        signing_key_id: signer.signing_key_id().to_string(),
        node_id: signer.node_id().to_string(),
        signed_at: OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default(),
        nonce: nonce.to_vec(),
        signature: Vec::new(),
    };
    let bytes = ackplane_protocol::claim_auth::claim_signing_bytes(
        tenant_id,
        repository_id,
        task_id,
        owner_id,
        &authentication,
    );
    authentication.signature = signer.sign(&bytes);
    authentication
}

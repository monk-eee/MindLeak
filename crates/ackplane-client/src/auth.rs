//! Producing a `ClaimAuthentication` a repository node can send with a claim
//! request (ADR-0096 clause 4).
//!
//! Signing itself is abstracted behind [`ClaimSigner`] rather than fixed to
//! one key-sourcing mechanism: ADR-0085 decision 2 and ADR-0100 decision 5
//! want the private key non-exportable through an OS credential facility
//! where one exists. [`CredentialFacilitySigner`] is that implementation
//! (Windows Credential Manager, macOS Keychain, or Linux Secret Service, via
//! the `keyring` crate) and is the seam a real federated deployment resolves
//! to by default. [`SeedSigner`] remains for tests and documented
//! non-hardened use -- a raw seed, sourced today from an environment
//! variable by the caller, the same posture already accepted here for
//! `MINDLEAK_LLM_API_KEY`. See
//! `gaps.d/the-node-signing-key-has-no-credential-facility-yet.md`.

use ed25519_dalek::{Signer, SigningKey};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

pub use ackplane_protocol::claim_auth::ClaimOperation;
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

/// Decode a 64-character hex string into a 32-byte Ed25519 seed. `None` if
/// the string is the wrong length or contains a non-hex character.
pub fn decode_seed(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 {
        return None;
    }
    let mut seed = [0u8; 32];
    for (index, byte) in seed.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(seed)
}

/// Encode a 32-byte Ed25519 seed as the same 64-character hex form
/// [`decode_seed`] reads back.
pub fn encode_seed(seed: &[u8; 32]) -> String {
    seed.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Why a [`CredentialFacilitySigner`] could not be built or used.
#[derive(Debug, thiserror::Error)]
pub enum CredentialFacilityError {
    #[error("the OS credential facility refused this request: {0}")]
    Facility(#[from] keyring::Error),
    #[error("the stored credential is not a 64-character hex-encoded seed")]
    MalformedSeed,
}

/// An Ed25519 [`ClaimSigner`] whose seed is held by the operating system's
/// credential facility -- Windows Credential Manager, macOS Keychain, or
/// Linux Secret Service, via the `keyring` crate -- rather than in this
/// process's own configuration (ADR-0085 decision 2, ADR-0100 decision 5).
/// This is the default seam a real federated deployment resolves to;
/// [`SeedSigner`] remains for tests and documented non-hardened use.
pub struct CredentialFacilitySigner {
    signing_key_id: String,
    node_id: String,
    key: SigningKey,
}

impl CredentialFacilitySigner {
    /// Read this node's signing seed from the OS credential facility, keyed
    /// by `service`/`account`. Never touches a file, an environment
    /// variable, or a command-line argument -- the seed exists only inside
    /// the OS-managed store and this process's memory for as long as this
    /// value is alive.
    pub fn load(
        signing_key_id: impl Into<String>,
        node_id: impl Into<String>,
        service: &str,
        account: &str,
    ) -> Result<Self, CredentialFacilityError> {
        let hex_seed = keyring::Entry::new(service, account)?.get_password()?;
        let seed = decode_seed(&hex_seed).ok_or(CredentialFacilityError::MalformedSeed)?;
        Ok(Self {
            signing_key_id: signing_key_id.into(),
            node_id: node_id.into(),
            key: SigningKey::from_bytes(&seed),
        })
    }

    /// Provision (or replace) the seed this node signs with. A one-time
    /// enrolment step, not part of the signing hot path: it writes once and
    /// every claim thereafter reads it back through [`load`](Self::load).
    pub fn store(
        service: &str,
        account: &str,
        seed: &[u8; 32],
    ) -> Result<(), CredentialFacilityError> {
        keyring::Entry::new(service, account)?.set_password(&encode_seed(seed))?;
        Ok(())
    }
}

impl ClaimSigner for CredentialFacilitySigner {
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
/// `ackplane-server`'s enrolment challenges use); the server durably consumes
/// each `(signing_key_id, nonce)` pair exactly once and bounds `signed_at` to
/// a clock-skew window, so a captured request cannot be replayed
/// indefinitely. `operation` binds this signature to the exact RPC and its
/// fields (ADR-0100 decision 10), so it can never verify for a different
/// operation or changed field values over the same task/owner.
pub fn authenticate(
    signer: &dyn ClaimSigner,
    tenant_id: &str,
    repository_id: &str,
    task_id: &str,
    owner_id: &str,
    operation: &ClaimOperation,
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
        operation,
        &authentication,
    );
    authentication.signature = signer.sign(&bytes);
    authentication
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_seed_survives_a_hex_round_trip() {
        let seed = [7u8; 32];
        assert_eq!(decode_seed(&encode_seed(&seed)), Some(seed));
    }

    #[test]
    fn a_seed_of_the_wrong_length_does_not_decode() {
        assert_eq!(decode_seed("ab"), None);
        assert_eq!(decode_seed(&"ab".repeat(40)), None);
    }

    #[test]
    fn a_non_hex_character_does_not_decode() {
        assert_eq!(decode_seed(&"zz".repeat(32)), None);
    }

    /// A unique account per test run: the OS credential facility is process-
    /// wide state, and a fixed name would collide across concurrent test
    /// runs on the same machine (this repository's own fleet regularly runs
    /// several worktrees at once).
    fn unique_account() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("mindleak-ackplane-client-test-{nanos}")
    }

    /// The real OS credential facility, round-tripped. Skips rather than
    /// fails when the facility itself is unreachable in this environment
    /// (e.g. a headless Linux CI runner with no Secret Service daemon) --
    /// that is an environment fact, not a defect in this signer, and this
    /// repository's own Postgres-gated tests skip the same way for the same
    /// reason.
    #[test]
    fn a_stored_seed_round_trips_through_the_real_credential_facility() {
        let service = "mindleak-ackplane-client-test";
        let account = unique_account();
        let seed = [42u8; 32];

        if let Err(error) = CredentialFacilitySigner::store(service, &account, &seed) {
            println!("skipped: OS credential facility unavailable in this environment: {error}");
            return;
        }

        let signer = CredentialFacilitySigner::load("key-1", "node-1", service, &account)
            .expect("the seed just stored should load back");
        let direct = SeedSigner::new("key-1", "node-1", &seed);
        assert_eq!(signer.sign(b"message"), direct.sign(b"message"));

        let _ = keyring::Entry::new(service, &account).and_then(|entry| entry.delete_password());
    }

    #[test]
    fn a_missing_credential_is_reported_as_a_facility_error_not_a_panic() {
        let service = "mindleak-ackplane-client-test";
        let account = format!("{}-never-stored", unique_account());
        match CredentialFacilitySigner::load("key-1", "node-1", service, &account) {
            Err(CredentialFacilityError::Facility(_)) => {}
            Err(CredentialFacilityError::MalformedSeed) => {
                panic!("a missing credential should be a Facility error, not MalformedSeed")
            }
            Ok(_) => panic!("nothing was ever stored under this account"),
        }
    }
}

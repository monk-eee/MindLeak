//! The `NodeSigner` capability contract (ADR-0100 decision 2).
//!
//! `NodeSigner` exposes only the closed set of operations a repository-side
//! caller needs: read the public identity, request a signature over an
//! already-computed digest, provision a successor key for rotation, and
//! retire/destroy a handle once Ackplane says it is safe to. There is no
//! `private_key()`, no seed export, no serialization of secret material, and
//! no way to sign arbitrary bytes without declaring the domain and binding
//! the provider checks before it signs.

use std::fmt;

/// The public half of a node's identity — safe to log, transmit, and persist.
#[derive(Clone, PartialEq, Eq)]
pub struct NodeIdentity {
    pub node_id: String,
    pub signing_key_id: String,
    /// Ed25519 public key bytes.
    pub public_key: [u8; 32],
    /// A short, human-comparable fingerprint derived from `public_key`.
    pub fingerprint: String,
}

impl fmt::Debug for NodeIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeIdentity")
            .field("node_id", &self.node_id)
            .field("signing_key_id", &self.signing_key_id)
            .field("fingerprint", &self.fingerprint)
            .field("public_key", &hex_encode(&self.public_key))
            .finish()
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The tenant/repository/node/key a signature request is scoped to. The
/// provider refuses to sign when this does not match its own active identity
/// (ADR-0100 decision 2/5) rather than signing whatever it is handed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SigningBinding {
    pub tenant_id: String,
    pub repository_id: String,
    pub node_id: String,
    pub key_id: String,
}

/// An opaque signature over a caller-supplied message digest. Not secret —
/// safe to transmit and log — but intentionally exposes no way to construct
/// one except through `NodeSigner::sign`.
#[derive(Clone, PartialEq, Eq)]
pub struct Signature(Vec<u8>);

impl Signature {
    pub(crate) fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes.to_vec())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Signature")
            .field(&hex_encode(&self.0))
            .finish()
    }
}

/// An opaque reference to a provisioned key inside a provider. Carries no key
/// material — it is only ever passed back to the same provider instance that
/// issued it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyHandle(pub(crate) String);

impl KeyHandle {
    /// Builds a handle from a `signing_key_id` seen on a [`NodeIdentity`].
    /// A `KeyHandle` carries no secret material — it is only ever meaningful
    /// to the same provider instance that minted the identity it names.
    pub fn from_signing_key_id(signing_key_id: impl Into<String>) -> Self {
        Self(signing_key_id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum NodeSignerError {
    #[error("signing binding {requested:?} does not match this provider's active identity")]
    BindingMismatch { requested: SigningBinding },
    #[error("key handle {0:?} is not known to this provider")]
    UnknownHandle(KeyHandle),
    #[error("a successor key has already been provisioned and not yet retired or destroyed")]
    SuccessorAlreadyProvisioned,
    #[error("provider refused: {0}")]
    ProviderRefused(String),
}

/// The one capability repository-side callers get. No implementor of this
/// trait may expose a method returning raw private key bytes.
pub trait NodeSigner: Send + Sync {
    /// The provider's current public identity.
    fn identity(&self) -> NodeIdentity;

    /// Sign `message_digest` (already hashed by the caller) for `domain`,
    /// after checking `binding` matches this provider's active identity.
    fn sign(
        &self,
        domain: &str,
        binding: &SigningBinding,
        message_digest: &[u8],
    ) -> Result<Signature, NodeSignerError>;

    /// Provision a successor key for rotation (ADR-0100 decision 8). Returns
    /// the successor's public identity; the private material never leaves
    /// the provider.
    fn provision_successor(&self) -> Result<NodeIdentity, NodeSignerError>;

    /// Mark a handle retired: it may still verify past signatures but must
    /// not be used to sign anything new.
    fn retire(&self, handle: &KeyHandle) -> Result<(), NodeSignerError>;

    /// Permanently destroy a handle's key material.
    fn destroy(&self, handle: &KeyHandle) -> Result<(), NodeSignerError>;
}

//! A development-only in-memory software [`crate::NodeSigner`] provider
//! (ADR-0100 decision 5's explicit dev carve-out). OS-backed providers
//! (Windows CNG, macOS Keychain/Secure Enclave, Linux PKCS#11/TPM) are
//! separate follow-on slices; this provider is not suitable for production
//! use.

use std::fmt;
use std::sync::Mutex;

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::signer::{KeyHandle, NodeIdentity, NodeSignerError, Signature, SigningBinding};
use crate::NodeSigner;

/// A signing key that can only be constructed by generating fresh random
/// material and whose `Debug` impl never renders its bytes. `ed25519-dalek`'s
/// `zeroize` feature makes the wrapped `SigningKey` zero its memory on drop.
struct SecretSigningKey(SigningKey);

impl fmt::Debug for SecretSigningKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretSigningKey(<redacted>)")
    }
}

#[derive(Debug)]
struct ActiveKey {
    key_id: String,
    tenant_id: String,
    repository_id: String,
    node_id: String,
    signing_key: SecretSigningKey,
}

impl ActiveKey {
    fn generate(tenant_id: &str, repository_id: &str, node_id: &str, key_id: &str) -> Self {
        let mut seed = [0u8; 32];
        getrandom::getrandom(&mut seed).expect("OS randomness source must be available");
        let signing_key = SigningKey::from_bytes(&seed);
        seed.zeroize_local();
        Self {
            key_id: key_id.to_string(),
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            node_id: node_id.to_string(),
            signing_key: SecretSigningKey(signing_key),
        }
    }

    fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.0.verifying_key()
    }

    fn identity(&self) -> NodeIdentity {
        let public_key = self.verifying_key().to_bytes();
        NodeIdentity {
            node_id: self.node_id.clone(),
            signing_key_id: self.key_id.clone(),
            public_key,
            fingerprint: fingerprint_of(&public_key),
        }
    }

    fn matches(&self, binding: &SigningBinding) -> bool {
        binding.tenant_id == self.tenant_id
            && binding.repository_id == self.repository_id
            && binding.node_id == self.node_id
            && binding.key_id == self.key_id
    }
}

/// Local helper trait so the raw seed buffer (a stack array, not covered by
/// `ed25519-dalek`'s own zeroize impl) is explicitly cleared too.
trait ZeroizeLocal {
    fn zeroize_local(&mut self);
}

impl ZeroizeLocal for [u8; 32] {
    fn zeroize_local(&mut self) {
        use zeroize::Zeroize;
        self.zeroize();
    }
}

fn fingerprint_of(public_key: &[u8; 32]) -> String {
    let digest = Sha256::digest(public_key);
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// A development-only in-memory software provider. The key exists only in
/// this process's memory for its lifetime; it is never written to disk,
/// serialized, or exported.
pub struct SoftwareProvider {
    state: Mutex<SoftwareProviderState>,
}

struct SoftwareProviderState {
    active: ActiveKey,
    successor: Option<ActiveKey>,
    retired: Vec<String>,
}

impl SoftwareProvider {
    /// Creates a fresh provider with a newly generated active key.
    pub fn generate(tenant_id: &str, repository_id: &str, node_id: &str) -> Self {
        let active = ActiveKey::generate(tenant_id, repository_id, node_id, "key-1");
        Self {
            state: Mutex::new(SoftwareProviderState {
                active,
                successor: None,
                retired: Vec::new(),
            }),
        }
    }
}

impl NodeSigner for SoftwareProvider {
    fn identity(&self) -> NodeIdentity {
        self.state
            .lock()
            .expect("provider state lock poisoned")
            .active
            .identity()
    }

    fn sign(
        &self,
        _domain: &str,
        binding: &SigningBinding,
        message_digest: &[u8],
    ) -> Result<Signature, NodeSignerError> {
        let state = self.state.lock().expect("provider state lock poisoned");
        let key = if state.active.matches(binding) {
            &state.active
        } else if let Some(successor) = state.successor.as_ref().filter(|s| s.matches(binding)) {
            successor
        } else {
            return Err(NodeSignerError::BindingMismatch {
                requested: binding.clone(),
            });
        };
        let signature = key.signing_key.0.sign(message_digest);
        Ok(Signature::from_bytes(signature.to_bytes()))
    }

    fn provision_successor(&self) -> Result<NodeIdentity, NodeSignerError> {
        let mut state = self.state.lock().expect("provider state lock poisoned");
        if state.successor.is_some() {
            return Err(NodeSignerError::SuccessorAlreadyProvisioned);
        }
        let next_id = format!("key-{}", state.retired.len() + 2);
        let successor = ActiveKey::generate(
            &state.active.tenant_id,
            &state.active.repository_id,
            &state.active.node_id,
            &next_id,
        );
        let identity = successor.identity();
        state.successor = Some(successor);
        Ok(identity)
    }

    fn retire(&self, handle: &KeyHandle) -> Result<(), NodeSignerError> {
        let mut state = self.state.lock().expect("provider state lock poisoned");
        if state.active.key_id == handle.as_str() {
            // Promote the successor (if any) to active; the next sign() call
            // naturally moves onto the new key.
            if let Some(successor) = state.successor.take() {
                let old = std::mem::replace(&mut state.active, successor);
                state.retired.push(old.key_id);
                return Ok(());
            }
            return Err(NodeSignerError::ProviderRefused(
                "cannot retire the active key with no successor provisioned".to_string(),
            ));
        }
        Err(NodeSignerError::UnknownHandle(handle.clone()))
    }

    fn destroy(&self, handle: &KeyHandle) -> Result<(), NodeSignerError> {
        let mut state = self.state.lock().expect("provider state lock poisoned");
        if state.retired.iter().any(|id| id == handle.as_str()) {
            state.retired.retain(|id| id != handle.as_str());
            return Ok(());
        }
        Err(NodeSignerError::UnknownHandle(handle.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::Verifier;

    fn binding_for(identity: &NodeIdentity) -> SigningBinding {
        SigningBinding {
            tenant_id: "tenant-a".to_string(),
            repository_id: "repo-a".to_string(),
            node_id: identity.node_id.clone(),
            key_id: identity.signing_key_id.clone(),
        }
    }

    #[test]
    fn identity_is_consistent_across_calls() {
        let provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        assert_eq!(provider.identity(), provider.identity());
    }

    #[test]
    fn sign_produces_a_verifiable_signature() {
        let provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let identity = provider.identity();
        let binding = binding_for(&identity);
        let digest = Sha256::digest(b"hello ackplane-node");

        let signature = provider.sign("claim", &binding, &digest).unwrap();

        let verifying_key = VerifyingKey::from_bytes(&identity.public_key).unwrap();
        let sig_bytes: [u8; 64] = signature.as_bytes().try_into().unwrap();
        verifying_key
            .verify(&digest, &ed25519_dalek::Signature::from_bytes(&sig_bytes))
            .expect("signature must verify against the identity's public key");
    }

    #[test]
    fn sign_refuses_a_mismatched_binding() {
        let provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let wrong_binding = SigningBinding {
            tenant_id: "tenant-b".to_string(),
            repository_id: "repo-a".to_string(),
            node_id: "node-a".to_string(),
            key_id: "key-1".to_string(),
        };
        let digest = Sha256::digest(b"hello");

        let result = provider.sign("claim", &wrong_binding, &digest);

        assert!(matches!(
            result,
            Err(NodeSignerError::BindingMismatch { .. })
        ));
    }

    #[test]
    fn debug_never_renders_secret_key_bytes() {
        let provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let state = provider.state.lock().unwrap();
        let rendered = format!("{:?}", state.active);
        assert!(!rendered.contains(&format!("{:?}", state.active.signing_key.0.to_bytes())));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn rotation_promotes_successor_and_retires_old_key() {
        let provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let original = provider.identity();

        let successor = provider.provision_successor().unwrap();
        assert_ne!(successor.signing_key_id, original.signing_key_id);

        let old_handle = KeyHandle::from_signing_key_id(original.signing_key_id.clone());
        provider.retire(&old_handle).unwrap();
        assert_eq!(provider.identity().signing_key_id, successor.signing_key_id);

        provider.destroy(&old_handle).unwrap();
        // Destroying an already-destroyed handle is refused, not silently ok.
        assert!(matches!(
            provider.destroy(&old_handle),
            Err(NodeSignerError::UnknownHandle(_))
        ));
    }
}

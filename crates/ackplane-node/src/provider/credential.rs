use std::{fmt, path::Path, sync::Arc};

use keyring::Entry;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use super::software::SoftwareProvider;
use crate::{
    enrol, EnrolmentError, EnrolmentRecord, KeyHandle, LockError, NodeIdentity, NodeProcessLock,
    NodeSigner, NodeSignerError, Signature, SigningBinding,
};

const SCHEME: &str = "credential-facility-software";
const SERVICE: &str = "mindleak-ackplane-node-software-v1";
const MAX_CREDENTIAL_BYTES: usize = 65_536;

#[derive(Deserialize, Serialize)]
struct StoredCredential {
    tenant_id: String,
    repository_id: String,
    node_id: String,
    key_id: String,
    seed: [u8; 32],
}

impl Drop for StoredCredential {
    fn drop(&mut self) {
        self.seed.zeroize();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CredentialProviderError {
    #[error("{0}")]
    Lock(#[from] LockError),
    #[error("{0}")]
    Enrollment(#[from] EnrolmentError),
    #[error(
        "a node identity or credential already exists; recover it instead of provisioning again"
    )]
    AlreadyProvisioned,
    #[error("identity_unavailable: invalid or mismatched provider binding")]
    InvalidBinding,
    #[error("identity_unavailable: unsupported provider or missing credential handle")]
    InvalidProvider,
    #[error("identity_unavailable: credential facility {0}")]
    Facility(&'static str),
    #[error("identity_unavailable: the stored credential is not a valid signing key")]
    MalformedCredential,
    #[error("identity_unavailable: the credential does not match the recorded public identity")]
    IdentityMismatch,
    #[error("the operating system could not supply randomness")]
    Random,
}

impl From<keyring::Error> for CredentialProviderError {
    fn from(error: keyring::Error) -> Self {
        match error {
            keyring::Error::NoEntry => {
                Self::Facility("entry is missing; restore the original credential")
            }
            _ => Self::Facility("is unavailable or refused the operation"),
        }
    }
}

/// An explicitly selected software signer whose seed persists in the OS credential
/// facility, never the repository. It is not a hardware non-exportable key.
pub struct CredentialProvider {
    binding: SigningBinding,
    identity: NodeIdentity,
    entry: Arc<Entry>,
    _owner: NodeProcessLock,
}

impl fmt::Debug for CredentialProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialProvider")
            .field("binding", &self.binding)
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

impl CredentialProvider {
    /// Create a new local provider identity. This does not enroll it on Ackplane.
    /// Existing enrollment or credentials are never replaced.
    pub fn provision(
        binding: SigningBinding,
        repository_state_dir: &Path,
    ) -> Result<Self, CredentialProviderError> {
        Self::provision_with(binding, repository_state_dir, Self::entry)
    }

    /// Recover only the credential and binding already recorded by this provider.
    pub fn recover(
        tenant_id: &str,
        repository_id: &str,
        repository_state_dir: &Path,
    ) -> Result<Self, CredentialProviderError> {
        Self::recover_with(tenant_id, repository_id, repository_state_dir, Self::entry)
    }

    fn entry(handle: &str) -> Result<Arc<Entry>, CredentialProviderError> {
        Ok(Arc::new(Entry::new(SERVICE, handle)?))
    }

    fn validate_binding(binding: &SigningBinding) -> Result<(), CredentialProviderError> {
        if [
            &binding.tenant_id,
            &binding.repository_id,
            &binding.node_id,
            &binding.key_id,
        ]
        .iter()
        .any(|value| value.trim().is_empty())
        {
            return Err(CredentialProviderError::InvalidBinding);
        }
        Ok(())
    }

    fn provision_with(
        binding: SigningBinding,
        repository_state_dir: &Path,
        credential: impl FnOnce(&str) -> Result<Arc<Entry>, CredentialProviderError>,
    ) -> Result<Self, CredentialProviderError> {
        Self::validate_binding(&binding)?;
        let owner = NodeProcessLock::acquire(repository_state_dir)?;
        match EnrolmentRecord::load(repository_state_dir) {
            Ok(_) => return Err(CredentialProviderError::AlreadyProvisioned),
            Err(EnrolmentError::NoRecord(_)) => {}
            Err(error) => return Err(error.into()),
        }
        let mut identifier = [0_u8; 16];
        getrandom::getrandom(&mut identifier).map_err(|_| CredentialProviderError::Random)?;
        let handle = identifier
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let entry = credential(&handle)?;
        match entry.get_password().map(Zeroizing::new) {
            Ok(_) => return Err(CredentialProviderError::AlreadyProvisioned),
            Err(keyring::Error::NoEntry) => {}
            Err(error) => return Err(error.into()),
        }
        let mut stored = StoredCredential {
            tenant_id: binding.tenant_id.clone(),
            repository_id: binding.repository_id.clone(),
            node_id: binding.node_id.clone(),
            key_id: binding.key_id.clone(),
            seed: [0_u8; 32],
        };
        getrandom::getrandom(&mut stored.seed).map_err(|_| CredentialProviderError::Random)?;
        let identity = SoftwareProvider::from_seed(&binding, &stored.seed).identity();
        let encoded = Zeroizing::new(
            serde_json::to_string(&stored)
                .map_err(|_| CredentialProviderError::MalformedCredential)?,
        );
        if encoded.len() > MAX_CREDENTIAL_BYTES {
            return Err(CredentialProviderError::InvalidBinding);
        }
        entry.set_password(&encoded)?;
        if let Err(error) = enrol(
            SCHEME,
            Some(&handle),
            &binding.tenant_id,
            &binding.repository_id,
            &identity,
            repository_state_dir,
        ) {
            entry.delete_password()?;
            return Err(error.into());
        }
        let provider = Self {
            binding,
            identity,
            entry,
            _owner: owner,
        };
        provider.read_signer()?;
        Ok(provider)
    }

    fn recover_with(
        tenant_id: &str,
        repository_id: &str,
        repository_state_dir: &Path,
        credential: impl FnOnce(&str) -> Result<Arc<Entry>, CredentialProviderError>,
    ) -> Result<Self, CredentialProviderError> {
        let owner = NodeProcessLock::acquire(repository_state_dir)?;
        let record = EnrolmentRecord::load(repository_state_dir)?;
        if record.tenant_id != tenant_id || record.repository_id != repository_id {
            return Err(CredentialProviderError::InvalidBinding);
        }
        let handle = record
            .provider_handle
            .as_deref()
            .ok_or(CredentialProviderError::InvalidProvider)?;
        if record.provider_scheme != SCHEME
            || handle.len() != 32
            || !handle
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(CredentialProviderError::InvalidProvider);
        }
        let binding = SigningBinding {
            tenant_id: record.tenant_id.clone(),
            repository_id: record.repository_id.clone(),
            node_id: record.node_id.clone(),
            key_id: record.signing_key_id.clone(),
        };
        Self::validate_binding(&binding)?;
        let identity = NodeIdentity {
            node_id: record.node_id.clone(),
            signing_key_id: record.signing_key_id.clone(),
            public_key: record
                .public_key
                .as_slice()
                .try_into()
                .map_err(|_| CredentialProviderError::IdentityMismatch)?,
            fingerprint: record.fingerprint.clone(),
        };
        let provider = Self {
            binding,
            identity,
            entry: credential(handle)?,
            _owner: owner,
        };
        provider.read_signer()?;
        Ok(provider)
    }

    fn read_signer(&self) -> Result<SoftwareProvider, CredentialProviderError> {
        let encoded = Zeroizing::new(self.entry.get_password()?);
        if encoded.len() > MAX_CREDENTIAL_BYTES {
            return Err(CredentialProviderError::MalformedCredential);
        }
        let stored: StoredCredential = serde_json::from_str(&encoded)
            .map_err(|_| CredentialProviderError::MalformedCredential)?;
        if stored.tenant_id != self.binding.tenant_id
            || stored.repository_id != self.binding.repository_id
            || stored.node_id != self.binding.node_id
            || stored.key_id != self.binding.key_id
        {
            return Err(CredentialProviderError::IdentityMismatch);
        }
        let signer = SoftwareProvider::from_seed(&self.binding, &stored.seed);
        if signer.identity() != self.identity {
            return Err(CredentialProviderError::IdentityMismatch);
        }
        Ok(signer)
    }
}

impl NodeSigner for CredentialProvider {
    fn identity(&self) -> NodeIdentity {
        self.identity.clone()
    }

    fn sign(
        &self,
        domain: &str,
        binding: &SigningBinding,
        message_digest: &[u8],
    ) -> Result<Signature, NodeSignerError> {
        if binding != &self.binding {
            return Err(NodeSignerError::BindingMismatch {
                requested: binding.clone(),
            });
        }
        self.read_signer()
            .map_err(|error| NodeSignerError::ProviderRefused(error.to_string()))?
            .sign(domain, binding, message_digest)
    }

    fn provision_successor(&self) -> Result<NodeIdentity, NodeSignerError> {
        Err(NodeSignerError::ProviderRefused(
            "persistent credential rotation is not implemented".to_string(),
        ))
    }

    fn retire(&self, _handle: &KeyHandle) -> Result<(), NodeSignerError> {
        Err(NodeSignerError::ProviderRefused(
            "persistent credential retirement is not implemented".to_string(),
        ))
    }

    fn destroy(&self, _handle: &KeyHandle) -> Result<(), NodeSignerError> {
        Err(NodeSignerError::ProviderRefused(
            "persistent credential destruction requires an explicit lifecycle operation"
                .to_string(),
        ))
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod platform_tests;

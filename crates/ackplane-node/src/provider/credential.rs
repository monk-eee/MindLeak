use std::{fmt, path::Path};

use crate::{
    EnrollmentActivation, EnrolmentError, KeyHandle, LockError, NodeIdentity, NodeSigner,
    NodeSignerError, Signature, SigningBinding,
};

mod candidate;
mod connection;
mod storage;

pub use candidate::CredentialCandidate;
use storage::CredentialStorage;

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
    #[error("identity_unavailable: this candidate has not been activated on Ackplane")]
    NotActivated,
    #[error("this credential is already activated; recover the enrolled provider")]
    AlreadyActivated,
    #[error("the activation challenge does not match this candidate's approved enrollment")]
    InvalidChallenge,
    #[error("no activation challenge has been recorded for this candidate")]
    NoChallenge,
    #[error("activation did not return a matching accepted request, key ID and receipt")]
    InvalidActivation,
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
    storage: CredentialStorage,
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
    /// Recover only the credential and binding already recorded by this provider.
    pub fn recover(
        tenant_id: &str,
        repository_id: &str,
        repository_state_dir: &Path,
    ) -> Result<Self, CredentialProviderError> {
        Self::from_storage(CredentialStorage::recover(
            tenant_id,
            repository_id,
            repository_state_dir,
            CredentialStorage::entry,
        )?)
    }

    #[cfg(test)]
    fn provision_with(
        binding: SigningBinding,
        repository_state_dir: &Path,
        credential: impl FnOnce(&str) -> Result<std::sync::Arc<keyring::Entry>, CredentialProviderError>,
    ) -> Result<Self, CredentialProviderError> {
        let mut candidate = CredentialCandidate::provision_with(
            &binding.tenant_id,
            &binding.repository_id,
            &binding.node_id,
            repository_state_dir,
            credential,
        )?;
        candidate.activation_proof(&ackplane_protocol::v1::EnrollmentChallenge {
            request_id: "request-test".to_string(),
            tenant_id: binding.tenant_id,
            repository_id: binding.repository_id,
            proposed_node_id: binding.node_id,
            public_key_fingerprint: candidate.identity().fingerprint,
            nonce: vec![7; 32],
            state: ackplane_protocol::v1::EnrollmentState::Approved as i32,
            ..Default::default()
        })?;
        candidate.accept_activation(&ackplane_protocol::v1::EnrollmentActivationResult {
            request_id: "request-test".to_string(),
            signing_key_id: binding.key_id,
            enrolment_receipt_id: "receipt-test".to_string(),
            state: ackplane_protocol::v1::EnrollmentState::Activating as i32,
            ..Default::default()
        })
    }

    #[cfg(test)]
    fn recover_with(
        tenant_id: &str,
        repository_id: &str,
        repository_state_dir: &Path,
        credential: impl FnOnce(&str) -> Result<std::sync::Arc<keyring::Entry>, CredentialProviderError>,
    ) -> Result<Self, CredentialProviderError> {
        Self::from_storage(CredentialStorage::recover(
            tenant_id,
            repository_id,
            repository_state_dir,
            credential,
        )?)
    }

    fn from_storage(storage: CredentialStorage) -> Result<Self, CredentialProviderError> {
        let record = &storage.record;
        let activation = record
            .activation
            .as_ref()
            .ok_or(CredentialProviderError::NotActivated)?;
        let binding = SigningBinding {
            tenant_id: record.tenant_id.clone(),
            repository_id: record.repository_id.clone(),
            node_id: record.node_id.clone(),
            key_id: activation.signing_key_id.clone(),
        };
        let identity = NodeIdentity {
            node_id: record.node_id.clone(),
            signing_key_id: activation.signing_key_id.clone(),
            public_key: record
                .public_key
                .as_slice()
                .try_into()
                .map_err(|_| CredentialProviderError::IdentityMismatch)?,
            fingerprint: record.fingerprint.clone(),
        };
        Ok(Self {
            binding,
            identity,
            storage,
        })
    }

    pub fn activation(&self) -> &EnrollmentActivation {
        self.storage
            .record
            .activation
            .as_ref()
            .expect("activated provider")
    }
}

impl NodeSigner for CredentialProvider {
    fn identity(&self) -> NodeIdentity {
        self.identity.clone()
    }

    fn sign(
        &self,
        _domain: &str,
        binding: &SigningBinding,
        message_digest: &[u8],
    ) -> Result<Signature, NodeSignerError> {
        if binding != &self.binding {
            return Err(NodeSignerError::BindingMismatch {
                requested: binding.clone(),
            });
        }
        self.storage
            .sign(message_digest)
            .map_err(|error| NodeSignerError::ProviderRefused(error.to_string()))
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
mod enrollment_tests;

#[cfg(test)]
mod recovery_tests;

#[cfg(test)]
mod server_tests;

#[cfg(test)]
mod platform_tests;

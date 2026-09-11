use std::{fmt, path::Path, sync::Arc};

use ackplane_protocol::{enrollment::activation_challenge_bytes, v1};
use ed25519_dalek::Signer;
use keyring::Entry;

use super::{CredentialProvider, CredentialProviderError, CredentialStorage};
use crate::{CandidateIdentity, EnrollmentActivation, EnrollmentChallengeRecord};

/// A provider-owned candidate key, not yet a runtime signing capability.
pub struct CredentialCandidate {
    storage: CredentialStorage,
}

impl fmt::Debug for CredentialCandidate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialCandidate")
            .field("identity", &self.identity())
            .finish_non_exhaustive()
    }
}

impl CredentialCandidate {
    /// Explicitly create a software key in the OS credential facility, never a seed file.
    pub fn provision(
        tenant_id: &str,
        repository_id: &str,
        node_id: &str,
        directory: &Path,
    ) -> Result<Self, CredentialProviderError> {
        Self::provision_with(
            tenant_id,
            repository_id,
            node_id,
            directory,
            CredentialStorage::entry,
        )
    }

    /// Restore an existing unactivated candidate; never create a replacement key.
    pub fn recover(
        tenant_id: &str,
        repository_id: &str,
        directory: &Path,
    ) -> Result<Self, CredentialProviderError> {
        Self::recover_with(
            tenant_id,
            repository_id,
            directory,
            CredentialStorage::entry,
        )
    }

    pub fn identity(&self) -> CandidateIdentity {
        self.storage.identity()
    }

    /// Persist a matching approved challenge before returning its signed proof.
    pub fn activation_proof(
        &mut self,
        challenge: &v1::EnrollmentChallenge,
    ) -> Result<v1::EnrollmentActivationProof, CredentialProviderError> {
        let record = &self.storage.record;
        if challenge.tenant_id != record.tenant_id
            || challenge.repository_id != record.repository_id
            || challenge.proposed_node_id != record.node_id
            || challenge.public_key_fingerprint != record.fingerprint
            || challenge.state() != v1::EnrollmentState::Approved
        {
            return Err(CredentialProviderError::InvalidChallenge);
        }
        self.storage.remember_challenge(EnrollmentChallengeRecord {
            request_id: challenge.request_id.clone(),
            nonce: challenge.nonce.clone(),
        })?;
        self.retry_activation_proof()
    }

    /// Reproduce only the recorded proof after an interrupted activation attempt.
    pub fn retry_activation_proof(
        &self,
    ) -> Result<v1::EnrollmentActivationProof, CredentialProviderError> {
        let record = &self.storage.record;
        let challenge = record
            .challenge
            .as_ref()
            .ok_or(CredentialProviderError::NoChallenge)?;
        let bytes = activation_challenge_bytes(
            &challenge.nonce,
            &challenge.request_id,
            &record.tenant_id,
            &record.repository_id,
            &record.node_id,
            &record.fingerprint,
        );
        let signature = self.storage.read_key()?.sign(&bytes).to_bytes().to_vec();
        Ok(v1::EnrollmentActivationProof {
            request_id: challenge.request_id.clone(),
            tenant_id: record.tenant_id.clone(),
            repository_id: record.repository_id.clone(),
            proposed_node_id: record.node_id.clone(),
            public_key_fingerprint: record.fingerprint.clone(),
            nonce: challenge.nonce.clone(),
            signature,
        })
    }

    /// Bind the configured authority's accepted result to this candidate's key.
    /// A subsequent authenticated connection must still verify current authority.
    pub fn accept_activation(
        self,
        result: &v1::EnrollmentActivationResult,
    ) -> Result<CredentialProvider, CredentialProviderError> {
        let challenge = self
            .storage
            .record
            .challenge
            .as_ref()
            .ok_or(CredentialProviderError::NoChallenge)?;
        if result.request_id != challenge.request_id
            || !matches!(
                result.state(),
                v1::EnrollmentState::Activating | v1::EnrollmentState::Active
            )
            || result.rejection_reason != 0
            || result.signing_key_id.trim().is_empty()
            || result.enrolment_receipt_id.trim().is_empty()
        {
            return Err(CredentialProviderError::InvalidActivation);
        }
        CredentialProvider::from_storage(self.storage.activate(EnrollmentActivation {
            request_id: result.request_id.clone(),
            signing_key_id: result.signing_key_id.clone(),
            enrolment_receipt_id: result.enrolment_receipt_id.clone(),
        })?)
    }

    pub(super) fn provision_with(
        tenant_id: &str,
        repository_id: &str,
        node_id: &str,
        directory: &Path,
        credential: impl FnOnce(&str) -> Result<Arc<Entry>, CredentialProviderError>,
    ) -> Result<Self, CredentialProviderError> {
        Ok(Self {
            storage: CredentialStorage::provision(
                tenant_id,
                repository_id,
                node_id,
                directory,
                credential,
            )?,
        })
    }

    pub(super) fn recover_with(
        tenant_id: &str,
        repository_id: &str,
        directory: &Path,
        credential: impl FnOnce(&str) -> Result<Arc<Entry>, CredentialProviderError>,
    ) -> Result<Self, CredentialProviderError> {
        let storage = CredentialStorage::recover(tenant_id, repository_id, directory, credential)?;
        if storage.record.activation.is_some() {
            return Err(CredentialProviderError::AlreadyActivated);
        }
        Ok(Self { storage })
    }
}

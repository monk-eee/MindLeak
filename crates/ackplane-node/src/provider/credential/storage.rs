use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use ackplane_protocol::enrollment::public_key_fingerprint;
use ed25519_dalek::SigningKey;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

use super::CredentialProviderError;
use crate::EnrollmentChallengeRecord;
use crate::{
    CandidateIdentity, EnrollmentActivation, EnrolmentError, EnrolmentRecord, NodeProcessLock,
};

const SCHEME: &str = "credential-facility-software";
const SERVICE: &str = "mindleak-ackplane-node-software-v1";
pub(super) const MAX_CREDENTIAL_BYTES: usize = 65_536;

#[derive(Deserialize, Serialize)]
struct StoredCredential {
    record: EnrolmentRecord,
    seed: [u8; 32],
}

impl Drop for StoredCredential {
    fn drop(&mut self) {
        self.seed.zeroize();
    }
}

pub(super) struct CredentialStorage {
    pub(super) record: EnrolmentRecord,
    entry: Arc<Entry>,
    directory: PathBuf,
    _owner: NodeProcessLock,
}

impl CredentialStorage {
    pub(super) fn entry(handle: &str) -> Result<Arc<Entry>, CredentialProviderError> {
        Ok(Arc::new(Entry::new(SERVICE, handle)?))
    }

    pub(super) fn provision(
        tenant_id: &str,
        repository_id: &str,
        node_id: &str,
        directory: &Path,
        credential: impl FnOnce(&str) -> Result<Arc<Entry>, CredentialProviderError>,
    ) -> Result<Self, CredentialProviderError> {
        if [tenant_id, repository_id, node_id]
            .iter()
            .any(|value| value.trim().is_empty())
        {
            return Err(CredentialProviderError::InvalidBinding);
        }
        let owner = NodeProcessLock::acquire(directory)?;
        match EnrolmentRecord::load(directory) {
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
        let mut seed = Zeroizing::new([0_u8; 32]);
        getrandom::getrandom(seed.as_mut()).map_err(|_| CredentialProviderError::Random)?;
        let public_key = SigningKey::from_bytes(&seed).verifying_key().to_bytes();
        let record = EnrolmentRecord {
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            node_id: node_id.to_string(),
            provider_scheme: SCHEME.to_string(),
            provider_handle: Some(handle.clone()),
            challenge: None,
            activation: None,
            public_key: public_key.to_vec(),
            fingerprint: public_key_fingerprint(&public_key),
        };
        let stored = StoredCredential {
            record: record.clone(),
            seed: *seed,
        };
        Self::store(&entry, &stored)?;
        if let Err(error) = record.create(directory) {
            entry.delete_password()?;
            return Err(error.into());
        }
        let storage = Self {
            record,
            entry,
            directory: directory.to_path_buf(),
            _owner: owner,
        };
        storage.read_key()?;
        Ok(storage)
    }

    pub(super) fn recover(
        tenant_id: &str,
        repository_id: &str,
        directory: &Path,
        credential: impl FnOnce(&str) -> Result<Arc<Entry>, CredentialProviderError>,
    ) -> Result<Self, CredentialProviderError> {
        let owner = NodeProcessLock::acquire(directory)?;
        let record = EnrolmentRecord::load(directory)?;
        if record.tenant_id != tenant_id
            || record.repository_id != repository_id
            || [tenant_id, repository_id, &record.node_id]
                .iter()
                .any(|value| value.trim().is_empty())
        {
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
        let entry = credential(handle)?;
        let mut storage = Self {
            record,
            entry,
            directory: directory.to_path_buf(),
            _owner: owner,
        };
        let stored = storage.load_credential()?;
        if let Some(challenge) = &stored.record.challenge {
            if challenge.request_id.trim().is_empty()
                || challenge.nonce.len() != 32
                || stored.record.activation.is_some()
            {
                return Err(CredentialProviderError::InvalidChallenge);
            }
        }
        if let Some(activation) = &stored.record.activation {
            Self::validate_activation(activation)?;
        }
        if stored.record != storage.record {
            let mut completed = storage.record.clone();
            completed.activation = stored.record.activation.clone();
            completed.challenge = stored.record.challenge.clone();
            let recorded_request = completed
                .activation
                .as_ref()
                .map(|activation| &activation.request_id)
                .or_else(|| {
                    completed
                        .challenge
                        .as_ref()
                        .map(|challenge| &challenge.request_id)
                });
            if storage.record.activation.is_some()
                || recorded_request.is_none()
                || storage
                    .record
                    .challenge
                    .as_ref()
                    .is_some_and(|challenge| Some(&challenge.request_id) != recorded_request)
                || completed != stored.record
            {
                return Err(CredentialProviderError::IdentityMismatch);
            }
            Self::verify_key(&stored)?;
            completed.replace(directory, &storage.record)?;
            storage.record = completed;
        }
        storage.read_key()?;
        EnrolmentRecord::recover(tenant_id, repository_id, &storage.identity(), directory)?;
        Ok(storage)
    }

    pub(super) fn identity(&self) -> CandidateIdentity {
        CandidateIdentity {
            node_id: self.record.node_id.clone(),
            public_key: self
                .record
                .public_key
                .as_slice()
                .try_into()
                .expect("validated public key"),
            fingerprint: self.record.fingerprint.clone(),
        }
    }

    fn load_credential(&self) -> Result<StoredCredential, CredentialProviderError> {
        let encoded = Zeroizing::new(self.entry.get_password()?);
        if encoded.len() > MAX_CREDENTIAL_BYTES {
            return Err(CredentialProviderError::MalformedCredential);
        }
        serde_json::from_str(&encoded).map_err(|_| CredentialProviderError::MalformedCredential)
    }

    fn verify_key(stored: &StoredCredential) -> Result<SigningKey, CredentialProviderError> {
        let key = SigningKey::from_bytes(&stored.seed);
        let public_key = key.verifying_key().to_bytes();
        if stored.record.public_key != public_key
            || stored.record.fingerprint != public_key_fingerprint(&public_key)
        {
            return Err(CredentialProviderError::IdentityMismatch);
        }
        Ok(key)
    }

    pub(super) fn read_key(&self) -> Result<SigningKey, CredentialProviderError> {
        let stored = self.load_credential()?;
        if stored.record != self.record {
            return Err(CredentialProviderError::IdentityMismatch);
        }
        Self::verify_key(&stored)
    }

    fn store(entry: &Entry, stored: &StoredCredential) -> Result<(), CredentialProviderError> {
        let encoded = Zeroizing::new(
            serde_json::to_string(stored)
                .map_err(|_| CredentialProviderError::MalformedCredential)?,
        );
        if encoded.len() > MAX_CREDENTIAL_BYTES {
            return Err(CredentialProviderError::InvalidBinding);
        }
        entry.set_password(&encoded)?;
        Ok(())
    }

    fn validate_activation(
        activation: &EnrollmentActivation,
    ) -> Result<(), CredentialProviderError> {
        if [
            &activation.request_id,
            &activation.signing_key_id,
            &activation.enrolment_receipt_id,
        ]
        .iter()
        .any(|value| value.trim().is_empty())
        {
            return Err(CredentialProviderError::InvalidBinding);
        }
        Ok(())
    }

    pub(super) fn activate(
        mut self,
        activation: EnrollmentActivation,
    ) -> Result<Self, CredentialProviderError> {
        if self.record.activation.is_some() {
            return Err(CredentialProviderError::AlreadyActivated);
        }
        Self::validate_activation(&activation)?;
        let mut next = self.record.clone();
        next.activation = Some(activation);
        next.challenge = None;
        self.publish(next)?;
        Ok(self)
    }

    pub(super) fn remember_challenge(
        &mut self,
        challenge: EnrollmentChallengeRecord,
    ) -> Result<(), CredentialProviderError> {
        if self.record.activation.is_some() {
            return Err(CredentialProviderError::AlreadyActivated);
        }
        if challenge.request_id.trim().is_empty()
            || challenge.nonce.len() != 32
            || self
                .record
                .challenge
                .as_ref()
                .is_some_and(|previous| previous.request_id != challenge.request_id)
        {
            return Err(CredentialProviderError::InvalidChallenge);
        }
        if self.record.challenge.as_ref() == Some(&challenge) {
            return Ok(());
        }
        let mut next = self.record.clone();
        next.challenge = Some(challenge);
        self.publish(next)
    }

    fn publish(&mut self, next: EnrolmentRecord) -> Result<(), CredentialProviderError> {
        let mut stored = self.load_credential()?;
        if stored.record != self.record {
            return Err(CredentialProviderError::IdentityMismatch);
        }
        Self::verify_key(&stored)?;
        stored.record = next;
        Self::store(&self.entry, &stored)?;
        stored.record.replace(&self.directory, &self.record)?;
        self.record = stored.record.clone();
        Ok(())
    }
}

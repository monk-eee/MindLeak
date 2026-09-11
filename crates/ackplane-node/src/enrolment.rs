//! Enrolment + restart identity recovery (ADR-0100 decision 7).
//!
//! On first enrolment, the non-secret half of a node's identity — never the
//! private key itself — is persisted beside the provider scheme and opaque
//! key handle. On restart, the current provider's identity is compared
//! against that persisted record before any stream is opened or claim is
//! acquired; a mismatch or missing record means `identity_unavailable`
//! rather than silently minting a replacement identity.
//!
//! This module owns only public metadata persistence and comparison.
//! Candidate proof generation and accepted activation binding belong to
//! `CredentialCandidate`; runtime recovery belongs to `CredentialProvider`.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::signer::CandidateIdentity;

const ENROLMENT_FILE_NAME: &str = "enrolment.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollmentActivation {
    pub request_id: String,
    pub signing_key_id: String,
    pub enrolment_receipt_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrollmentChallengeRecord {
    pub request_id: String,
    pub nonce: Vec<u8>,
}

/// The non-secret binding metadata ADR-0085 decision 6 requires, plus the
/// provider scheme ADR-0100 decision 7 adds. No field here is ever the
/// private key or material that could reconstruct it — this type simply has
/// no such field, which is what fresh_enrolment_persists_only_non_secret_fields
/// documents rather than a heuristic string search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrolmentRecord {
    pub tenant_id: String,
    pub repository_id: String,
    pub node_id: String,
    pub provider_scheme: String,
    /// An opaque provider account, never a private seed. Required by persistent providers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub challenge: Option<EnrollmentChallengeRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation: Option<EnrollmentActivation>,
    pub public_key: Vec<u8>,
    pub fingerprint: String,
}

impl EnrolmentRecord {
    pub(crate) fn load(repository_state_dir: &Path) -> Result<Self, EnrolmentError> {
        let path = record_path(repository_state_dir);
        let bytes = fs::read(&path).map_err(|source| {
            if source.kind() == io::ErrorKind::NotFound {
                EnrolmentError::NoRecord(path.clone())
            } else {
                EnrolmentError::Io {
                    path: path.clone(),
                    source,
                }
            }
        })?;
        serde_json::from_slice(&bytes).map_err(|source| EnrolmentError::Parse { path, source })
    }

    pub(crate) fn create(&self, repository_state_dir: &Path) -> Result<(), EnrolmentError> {
        let path = record_path(repository_state_dir);
        if path.exists() {
            return Err(EnrolmentError::AlreadyEnrolled(path));
        }
        fs::create_dir_all(repository_state_dir).map_err(|source| EnrolmentError::Io {
            path: repository_state_dir.to_path_buf(),
            source,
        })?;
        self.persist(repository_state_dir, false)
    }

    pub(crate) fn recover(
        tenant_id: &str,
        repository_id: &str,
        identity: &CandidateIdentity,
        repository_state_dir: &Path,
    ) -> Result<Self, EnrolmentError> {
        let record = Self::load(repository_state_dir)?;
        if !record.matches(identity, tenant_id, repository_id) {
            return Err(EnrolmentError::Mismatch(record_path(repository_state_dir)));
        }
        Ok(record)
    }

    pub(crate) fn replace(
        &self,
        repository_state_dir: &Path,
        previous: &Self,
    ) -> Result<(), EnrolmentError> {
        let path = record_path(repository_state_dir);
        if Self::load(repository_state_dir)? != *previous {
            return Err(EnrolmentError::Mismatch(path));
        }
        self.persist(repository_state_dir, true)
    }

    fn persist(&self, repository_state_dir: &Path, replace: bool) -> Result<(), EnrolmentError> {
        let path = record_path(repository_state_dir);
        let write = || -> io::Result<()> {
            let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
            let mut temporary = tempfile::NamedTempFile::new_in(repository_state_dir)?;
            temporary.write_all(&bytes)?;
            temporary.as_file().sync_all()?;
            if replace {
                temporary.persist(&path).map_err(|error| error.error)?;
            } else {
                temporary
                    .persist_noclobber(&path)
                    .map_err(|error| error.error)?;
            }
            Ok(())
        };
        write().map_err(|source| {
            if source.kind() == io::ErrorKind::AlreadyExists {
                EnrolmentError::AlreadyEnrolled(path.clone())
            } else {
                EnrolmentError::Io { path, source }
            }
        })
    }

    fn matches(&self, identity: &CandidateIdentity, tenant_id: &str, repository_id: &str) -> bool {
        self.tenant_id == tenant_id
            && self.repository_id == repository_id
            && self.node_id == identity.node_id
            && self.public_key == identity.public_key
            && self.fingerprint == identity.fingerprint
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EnrolmentError {
    #[error("an enrolment record already exists at {0}; this is a restart, not a first enrolment")]
    AlreadyEnrolled(PathBuf),
    #[error("identity_unavailable: no enrolment record exists at {0}")]
    NoRecord(PathBuf),
    #[error("identity_unavailable: the current provider's identity does not match the enrolment record at {0}")]
    Mismatch(PathBuf),
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not parse the enrolment record at {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

fn record_path(repository_state_dir: &Path) -> PathBuf {
    repository_state_dir.join(ENROLMENT_FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::software::SoftwareProvider;
    use crate::NodeSigner;

    fn identity(provider: &SoftwareProvider) -> CandidateIdentity {
        let identity = provider.identity();
        CandidateIdentity {
            node_id: identity.node_id,
            public_key: identity.public_key,
            fingerprint: identity.fingerprint,
        }
    }

    fn record(identity: &CandidateIdentity) -> EnrolmentRecord {
        EnrolmentRecord {
            tenant_id: "tenant-a".to_string(),
            repository_id: "repo-a".to_string(),
            node_id: identity.node_id.clone(),
            provider_scheme: "software-dev".to_string(),
            provider_handle: None,
            challenge: None,
            activation: None,
            public_key: identity.public_key.to_vec(),
            fingerprint: identity.fingerprint.clone(),
        }
    }

    #[test]
    fn fresh_enrolment_persists_only_non_secret_fields() {
        let dir = tempfile::tempdir().unwrap();
        let provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let identity = identity(&provider);

        let record = record(&identity);
        record.create(dir.path()).unwrap();

        assert_eq!(record.node_id, identity.node_id);
        assert_eq!(record.fingerprint, identity.fingerprint);

        // Round-tripping through EnrolmentRecord itself is the structural
        // guarantee: the type has no field that could hold private key
        // material, so whatever was written is exactly this record.
        let raw = fs::read_to_string(record_path(dir.path())).unwrap();
        let reparsed: EnrolmentRecord = serde_json::from_str(&raw).unwrap();
        assert_eq!(reparsed, record);
    }

    #[test]
    fn enrolling_twice_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let identity = identity(&provider);
        let record = record(&identity);
        record.create(dir.path()).unwrap();

        let result = record.create(dir.path());

        assert!(matches!(result, Err(EnrolmentError::AlreadyEnrolled(_))));
    }

    // A broken enrollment symlink used to look absent and let provisioning write through it.
    #[cfg(unix)]
    #[test]
    fn enrollment_preserves_an_existing_dangling_symlink() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("missing-record.json");
        let path = record_path(directory.path());
        std::os::unix::fs::symlink(&target, &path).unwrap();
        let provider = SoftwareProvider::generate("tenant-test", "repo-test", "node-test");

        let result = record(&identity(&provider)).create(directory.path());

        assert!(matches!(result, Err(EnrolmentError::AlreadyEnrolled(_))));
        assert_eq!(fs::read_link(&path).unwrap(), target);
        assert!(
            !target.exists(),
            "provisioning must not create the symlink's target"
        );
    }

    #[test]
    fn restart_with_matching_identity_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let identity = identity(&provider);
        record(&identity).create(dir.path()).unwrap();

        let recovered =
            EnrolmentRecord::recover("tenant-a", "repo-a", &identity, dir.path()).unwrap();

        assert_eq!(recovered.node_id, identity.node_id);
    }

    #[test]
    fn restart_with_no_record_is_identity_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let identity = identity(&provider);

        let result = EnrolmentRecord::recover("tenant-a", "repo-a", &identity, dir.path());

        assert!(matches!(result, Err(EnrolmentError::NoRecord(_))));
    }

    #[test]
    fn restart_with_a_mismatched_fingerprint_is_identity_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let original_provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let original_identity = identity(&original_provider);
        record(&original_identity).create(dir.path()).unwrap();

        // A different provider instance -- as if the key material were lost
        // and silently replaced -- reports a different public key and
        // fingerprint for what claims to be the same node id.
        let different_provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let different_identity = identity(&different_provider);

        let result =
            EnrolmentRecord::recover("tenant-a", "repo-a", &different_identity, dir.path());

        assert!(matches!(result, Err(EnrolmentError::Mismatch(_))));
    }
}

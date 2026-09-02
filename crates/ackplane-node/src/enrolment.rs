//! Enrolment + restart identity recovery (ADR-0100 decision 7).
//!
//! On first enrolment, the non-secret half of a node's identity — never the
//! private key itself — is persisted beside the provider scheme and opaque
//! key handle. On restart, the current provider's identity is compared
//! against that persisted record before any stream is opened or claim is
//! acquired; a mismatch or missing record means `identity_unavailable`
//! rather than silently minting a replacement identity.
//!
//! Submitting the public key/fingerprint to Ackplane itself (the other half
//! of decision 7) is a separate follow-on once a real Ackplane client is
//! wired into this crate — this module only owns the local persistence and
//! comparison ADR-0100 describes.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::signer::NodeIdentity;

const ENROLMENT_FILE_NAME: &str = "enrolment.json";

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
    pub signing_key_id: String,
    pub public_key: Vec<u8>,
    pub fingerprint: String,
}

impl EnrolmentRecord {
    fn matches(&self, identity: &NodeIdentity, tenant_id: &str, repository_id: &str) -> bool {
        self.tenant_id == tenant_id
            && self.repository_id == repository_id
            && self.node_id == identity.node_id
            && self.signing_key_id == identity.signing_key_id
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

/// First enrolment: captures `identity`'s public identity and persists it
/// alongside `provider_scheme`, refusing if a record already exists (a
/// restart should call [`recover`], not enrol again).
pub fn enrol(
    provider_scheme: &str,
    tenant_id: &str,
    repository_id: &str,
    identity: &NodeIdentity,
    repository_state_dir: &Path,
) -> Result<EnrolmentRecord, EnrolmentError> {
    let path = record_path(repository_state_dir);
    if path.exists() {
        return Err(EnrolmentError::AlreadyEnrolled(path));
    }
    fs::create_dir_all(repository_state_dir).map_err(|source| EnrolmentError::Io {
        path: repository_state_dir.to_path_buf(),
        source,
    })?;
    let record = EnrolmentRecord {
        tenant_id: tenant_id.to_string(),
        repository_id: repository_id.to_string(),
        node_id: identity.node_id.clone(),
        provider_scheme: provider_scheme.to_string(),
        signing_key_id: identity.signing_key_id.clone(),
        public_key: identity.public_key.to_vec(),
        fingerprint: identity.fingerprint.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&record).expect("EnrolmentRecord always serializes");
    fs::write(&path, bytes).map_err(|source| EnrolmentError::Io {
        path: path.clone(),
        source,
    })?;
    Ok(record)
}

/// Restart recovery: resolves the persisted record and compares it against
/// `identity` (the current provider's own reported identity) before letting
/// a caller open a stream or acquire a claim. Returns `identity_unavailable`
/// (as [`EnrolmentError::NoRecord`]/[`EnrolmentError::Mismatch`]) rather than
/// silently minting a replacement identity.
pub fn recover(
    tenant_id: &str,
    repository_id: &str,
    identity: &NodeIdentity,
    repository_state_dir: &Path,
) -> Result<EnrolmentRecord, EnrolmentError> {
    let path = record_path(repository_state_dir);
    if !path.exists() {
        return Err(EnrolmentError::NoRecord(path));
    }
    let bytes = fs::read(&path).map_err(|source| EnrolmentError::Io {
        path: path.clone(),
        source,
    })?;
    let record: EnrolmentRecord =
        serde_json::from_slice(&bytes).map_err(|source| EnrolmentError::Parse {
            path: path.clone(),
            source,
        })?;
    if !record.matches(identity, tenant_id, repository_id) {
        return Err(EnrolmentError::Mismatch(path));
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::software::SoftwareProvider;
    use crate::NodeSigner;

    #[test]
    fn fresh_enrolment_persists_only_non_secret_fields() {
        let dir = tempfile::tempdir().unwrap();
        let provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let identity = provider.identity();

        let record = enrol("software-dev", "tenant-a", "repo-a", &identity, dir.path()).unwrap();

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
        let identity = provider.identity();
        enrol("software-dev", "tenant-a", "repo-a", &identity, dir.path()).unwrap();

        let result = enrol("software-dev", "tenant-a", "repo-a", &identity, dir.path());

        assert!(matches!(result, Err(EnrolmentError::AlreadyEnrolled(_))));
    }

    #[test]
    fn restart_with_matching_identity_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let identity = provider.identity();
        enrol("software-dev", "tenant-a", "repo-a", &identity, dir.path()).unwrap();

        let recovered = recover("tenant-a", "repo-a", &identity, dir.path()).unwrap();

        assert_eq!(recovered.node_id, identity.node_id);
    }

    #[test]
    fn restart_with_no_record_is_identity_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let identity = provider.identity();

        let result = recover("tenant-a", "repo-a", &identity, dir.path());

        assert!(matches!(result, Err(EnrolmentError::NoRecord(_))));
    }

    #[test]
    fn restart_with_a_mismatched_fingerprint_is_identity_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let original_provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let original_identity = original_provider.identity();
        enrol(
            "software-dev",
            "tenant-a",
            "repo-a",
            &original_identity,
            dir.path(),
        )
        .unwrap();

        // A different provider instance -- as if the key material were lost
        // and silently replaced -- reports a different public key and
        // fingerprint for what claims to be the same node id.
        let different_provider = SoftwareProvider::generate("tenant-a", "repo-a", "node-a");
        let different_identity = different_provider.identity();

        let result = recover("tenant-a", "repo-a", &different_identity, dir.path());

        assert!(matches!(result, Err(EnrolmentError::Mismatch(_))));
    }
}

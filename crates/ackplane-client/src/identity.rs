//! A repository's own candidate node identity, persisted locally so a
//! process can ask `CheckEnrollmentStatus` about itself without a human
//! re-supplying its identity every time (ADR-0122; closes
//! `gaps.d/ackplane-client-cannot-detect-unenrolled-repositories.md`).
//!
//! Deliberately duck-types the JSON `register-me request`/`activate` already
//! write to `<key-path>.enrollment.json` (tenant_id, repository_id, node_id,
//! public_key_fingerprint) instead of sharing a Rust type across the
//! ackplane-server/ackplane-client boundary: the two crates agree on a file
//! format, not a dependency edge back from this crate into the server's.

use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey};
use serde::Deserialize;
use thiserror::Error;

use ackplane_protocol::enrollment_status_auth::{
    enrollment_status_signing_bytes, EnrollmentStatusOperation,
};
use ackplane_protocol::v1::{EnrollmentStatusAuthentication, EnrollmentStatusRequest};

/// Where a repository's own candidate identity lives absent an override --
/// the same `.mindleak/` directory `.gitignore` already excludes for
/// MindLeak's own local graph database, so following the default never
/// risks committing a private key.
pub const DEFAULT_KEY_PATH: &str = ".mindleak/ackplane-node.key";

/// Overrides [`DEFAULT_KEY_PATH`], mirroring `MINDLEAK_ACKPLANE_ENDPOINT`'s
/// repository-local-override style (ADR-0082 decision 3).
pub const KEY_PATH_ENV: &str = "MINDLEAK_ACKPLANE_KEY_PATH";

/// The fields `register-me request`/`activate` persist to
/// `<key_path>.enrollment.json` that a status check needs. Not every field
/// register-me saves is named here -- `grpc_endpoint` is a CLI convenience,
/// not part of this repository's identity.
#[derive(Debug, Deserialize)]
struct SavedRequest {
    tenant_id: String,
    repository_id: String,
    node_id: String,
    public_key_fingerprint: String,
}

/// This repository's own enrolled (or candidate) identity: who it claims to
/// be, not proof that the arbiter agrees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateIdentity {
    pub tenant_id: String,
    pub repository_id: String,
    pub node_id: String,
    pub public_key_fingerprint: String,
}

/// Why a repository's candidate identity could not be loaded. Every variant
/// is treated identically by a caller that only wants to know "is there an
/// identity to ask about" -- a repository that has never enrolled and one
/// whose saved state is unreadable both answer that question the same way.
#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("no candidate identity at {0}: `register-me request` has not run here yet")]
    NotFound(PathBuf),
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("{0}: {1}")]
    Malformed(PathBuf, serde_json::Error),
    #[error("{0} does not hold a 32-byte Ed25519 seed")]
    InvalidKey(PathBuf),
    #[error("could not generate a nonce: {0}")]
    Random(String),
}

/// Resolve the key path this repository's identity lives at:
/// `environment`'s [`KEY_PATH_ENV`] value if declared, else
/// [`DEFAULT_KEY_PATH`].
pub fn resolve_key_path<F>(environment: &F) -> PathBuf
where
    F: Fn(&str) -> Option<String>,
{
    environment(KEY_PATH_ENV)
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_KEY_PATH))
}

fn sidecar_path(key_path: &Path) -> PathBuf {
    let mut path = key_path.as_os_str().to_owned();
    path.push(".enrollment.json");
    PathBuf::from(path)
}

/// Load the identity `register-me request`/`activate` persisted at
/// `key_path` and its `<key_path>.enrollment.json` sidecar.
/// [`IdentityError::NotFound`] is the correct, cheap answer for
/// `FederationReadiness::NotEnrolled` without ever asking the arbiter: a
/// repository that never wrote this sidecar has never run the enrolment
/// ceremony.
pub fn load_candidate_identity(
    key_path: &Path,
) -> Result<(CandidateIdentity, SigningKey), IdentityError> {
    let sidecar = sidecar_path(key_path);
    if !sidecar.exists() {
        return Err(IdentityError::NotFound(sidecar));
    }
    let raw = std::fs::read(&sidecar).map_err(|error| IdentityError::Io(sidecar.clone(), error))?;
    let saved: SavedRequest =
        serde_json::from_slice(&raw).map_err(|error| IdentityError::Malformed(sidecar, error))?;

    let key_bytes = std::fs::read(key_path)
        .map_err(|error| IdentityError::Io(key_path.to_path_buf(), error))?;
    let seed = <[u8; 32]>::try_from(key_bytes.as_slice())
        .map_err(|_| IdentityError::InvalidKey(key_path.to_path_buf()))?;

    Ok((
        CandidateIdentity {
            tenant_id: saved.tenant_id,
            repository_id: saved.repository_id,
            node_id: saved.node_id,
            public_key_fingerprint: saved.public_key_fingerprint,
        },
        SigningKey::from_bytes(&seed),
    ))
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("well-known RFC3339 formatting never fails")
}

/// Build a freshly signed `CheckEnrollmentStatus` request proving possession
/// of `identity`'s key -- the exact bytes ADR-0122's
/// `enrollment_status_signing_bytes` defines, signed with `signing_key`. A
/// fresh random nonce each call satisfies the server's anti-replay
/// uniqueness check (`consume_status_nonce`) without coordinating with any
/// prior request.
pub fn signed_status_request(
    identity: &CandidateIdentity,
    signing_key: &SigningKey,
) -> Result<EnrollmentStatusRequest, IdentityError> {
    let mut nonce = [0_u8; 16];
    getrandom::getrandom(&mut nonce).map_err(|error| IdentityError::Random(error.to_string()))?;

    let mut authentication = EnrollmentStatusAuthentication {
        node_id: identity.node_id.clone(),
        key_fingerprint: identity.public_key_fingerprint.clone(),
        signed_at: now_rfc3339(),
        nonce: nonce.to_vec(),
        signature: Vec::new(),
    };
    let bytes = enrollment_status_signing_bytes(
        &identity.tenant_id,
        &identity.repository_id,
        EnrollmentStatusOperation::Check,
        &authentication,
    );
    authentication.signature = signing_key.sign(&bytes).to_bytes().to_vec();

    Ok(EnrollmentStatusRequest {
        tenant_id: identity.tenant_id.clone(),
        repository_id: identity.repository_id.clone(),
        candidate_node_id: identity.node_id.clone(),
        candidate_key_fingerprint: identity.public_key_fingerprint.clone(),
        authentication: Some(authentication),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, Verifier};

    fn temp_key_path(label: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("ackplane-identity-test-{label}-{nanos}.key"));
        path
    }

    fn signing_key_bytes() -> [u8; 32] {
        [7_u8; 32]
    }

    #[test]
    fn resolve_key_path_defaults_when_unset() {
        assert_eq!(
            resolve_key_path(&|_: &str| None),
            PathBuf::from(DEFAULT_KEY_PATH)
        );
    }

    #[test]
    fn resolve_key_path_honours_the_override() {
        let env = |name: &str| (name == KEY_PATH_ENV).then(|| "/custom/node.key".to_string());
        assert_eq!(resolve_key_path(&env), PathBuf::from("/custom/node.key"));
    }

    #[test]
    fn a_repository_that_never_enrolled_reports_not_found() {
        let path = temp_key_path("missing");
        let error = load_candidate_identity(&path).expect_err("no sidecar was written");
        assert!(matches!(error, IdentityError::NotFound(_)));
    }

    #[test]
    fn a_saved_identity_and_key_round_trip() {
        let path = temp_key_path("round-trip");
        std::fs::write(&path, signing_key_bytes()).unwrap();
        let sidecar = sidecar_path(&path);
        std::fs::write(
            &sidecar,
            serde_json::json!({
                "request_id": "request-1",
                "tenant_id": "tenant-1",
                "repository_id": "repository-1",
                "node_id": "node-1",
                "public_key_fingerprint": "fingerprint-1",
                "grpc_endpoint": "http://127.0.0.1:8443",
            })
            .to_string(),
        )
        .unwrap();

        let (identity, signing_key) =
            load_candidate_identity(&path).expect("a saved identity should load");
        assert_eq!(
            identity,
            CandidateIdentity {
                tenant_id: "tenant-1".to_string(),
                repository_id: "repository-1".to_string(),
                node_id: "node-1".to_string(),
                public_key_fingerprint: "fingerprint-1".to_string(),
            }
        );
        assert_eq!(signing_key.to_bytes(), signing_key_bytes());

        std::fs::remove_file(&path).ok();
        std::fs::remove_file(&sidecar).ok();
    }

    #[test]
    fn a_malformed_sidecar_is_reported_not_panicked_on() {
        let path = temp_key_path("malformed");
        std::fs::write(&path, signing_key_bytes()).unwrap();
        let sidecar = sidecar_path(&path);
        std::fs::write(&sidecar, b"not json").unwrap();

        let error = load_candidate_identity(&path).expect_err("malformed JSON must not parse");
        assert!(matches!(error, IdentityError::Malformed(_, _)));

        std::fs::remove_file(&path).ok();
        std::fs::remove_file(&sidecar).ok();
    }

    #[test]
    fn a_key_file_of_the_wrong_length_is_reported_not_panicked_on() {
        let path = temp_key_path("short-key");
        std::fs::write(&path, [1_u8; 4]).unwrap();
        let sidecar = sidecar_path(&path);
        std::fs::write(
            &sidecar,
            serde_json::json!({
                "request_id": "request-1",
                "tenant_id": "tenant-1",
                "repository_id": "repository-1",
                "node_id": "node-1",
                "public_key_fingerprint": "fingerprint-1",
                "grpc_endpoint": "http://127.0.0.1:8443",
            })
            .to_string(),
        )
        .unwrap();

        let error = load_candidate_identity(&path).expect_err("a 4-byte seed must not parse");
        assert!(matches!(error, IdentityError::InvalidKey(_)));

        std::fs::remove_file(&path).ok();
        std::fs::remove_file(&sidecar).ok();
    }

    #[test]
    fn signed_status_request_carries_a_verifiable_signature() {
        let identity = CandidateIdentity {
            tenant_id: "tenant-1".to_string(),
            repository_id: "repository-1".to_string(),
            node_id: "node-1".to_string(),
            public_key_fingerprint: "fingerprint-1".to_string(),
        };
        let signing_key = SigningKey::from_bytes(&signing_key_bytes());

        let request = signed_status_request(&identity, &signing_key)
            .expect("signing must succeed with a real key");
        let authentication = request.authentication.expect("authentication must be set");
        let bytes = enrollment_status_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            EnrollmentStatusOperation::Check,
            &authentication,
        );
        let signature =
            Signature::from_slice(&authentication.signature).expect("a 64-byte signature");
        signing_key
            .verifying_key()
            .verify(&bytes, &signature)
            .expect("the signature must verify against the signed bytes");
    }

    #[test]
    fn two_signed_requests_never_reuse_a_nonce() {
        let identity = CandidateIdentity {
            tenant_id: "tenant-1".to_string(),
            repository_id: "repository-1".to_string(),
            node_id: "node-1".to_string(),
            public_key_fingerprint: "fingerprint-1".to_string(),
        };
        let signing_key = SigningKey::from_bytes(&signing_key_bytes());
        let first = signed_status_request(&identity, &signing_key).unwrap();
        let second = signed_status_request(&identity, &signing_key).unwrap();
        assert_ne!(
            first.authentication.unwrap().nonce,
            second.authentication.unwrap().nonce
        );
    }
}

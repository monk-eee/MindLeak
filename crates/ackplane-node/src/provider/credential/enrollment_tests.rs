use std::sync::Arc;

use ackplane_protocol::v1;
use keyring::Entry;

use super::{tests::credential, CredentialCandidate, CredentialProvider, CredentialProviderError};
use crate::{EnrolmentRecord, NodeSigner};

pub(super) fn candidate() -> (tempfile::TempDir, Arc<Entry>, CredentialCandidate) {
    let directory = tempfile::tempdir().unwrap();
    let entry = credential();
    let candidate = CredentialCandidate::provision_with(
        "tenant-test",
        "repo-test",
        "node-test",
        directory.path(),
        |_| Ok(entry.clone()),
    )
    .unwrap();
    (directory, entry, candidate)
}

pub(super) fn challenge(candidate: &CredentialCandidate) -> v1::EnrollmentChallenge {
    v1::EnrollmentChallenge {
        request_id: "request-assigned".to_string(),
        tenant_id: "tenant-test".to_string(),
        repository_id: "repo-test".to_string(),
        proposed_node_id: "node-test".to_string(),
        public_key_fingerprint: candidate.identity().fingerprint,
        nonce: vec![9; 32],
        state: v1::EnrollmentState::Approved as i32,
        ..Default::default()
    }
}

pub(super) fn response() -> v1::EnrollmentActivationResult {
    v1::EnrollmentActivationResult {
        request_id: "request-assigned".to_string(),
        signing_key_id: "authority-assigned-key".to_string(),
        enrolment_receipt_id: "authority-assigned-receipt".to_string(),
        state: v1::EnrollmentState::Activating as i32,
        ..Default::default()
    }
}

#[test]
fn candidate_cannot_be_opened_as_a_runtime_signer() {
    let (directory, entry, candidate) = candidate();
    assert!(matches!(
        candidate.retry_activation_proof(),
        Err(CredentialProviderError::NoChallenge)
    ));
    assert!(matches!(
        candidate.accept_activation(&response()),
        Err(CredentialProviderError::NoChallenge)
    ));
    assert!(matches!(
        CredentialProvider::recover_with("tenant-test", "repo-test", directory.path(), |_| Ok(
            entry.clone()
        )),
        Err(CredentialProviderError::NotActivated),
    ));
}

#[test]
fn activation_binds_the_authority_key_and_receipt_without_replacing_the_candidate_key() {
    let (directory, entry, mut candidate) = candidate();
    let identity = candidate.identity();
    candidate.activation_proof(&challenge(&candidate)).unwrap();
    let provider = candidate.accept_activation(&response()).unwrap();
    assert_eq!(provider.identity().public_key, identity.public_key);
    assert_eq!(
        provider.identity().signing_key_id,
        response().signing_key_id
    );
    assert_eq!(
        provider.activation().enrolment_receipt_id,
        response().enrolment_receipt_id
    );
    drop(provider);
    assert!(matches!(
        CredentialCandidate::recover_with("tenant-test", "repo-test", directory.path(), |_| Ok(
            entry.clone()
        )),
        Err(CredentialProviderError::AlreadyActivated)
    ));
    let restored =
        CredentialProvider::recover_with("tenant-test", "repo-test", directory.path(), |_| {
            Ok(entry.clone())
        })
        .unwrap();
    assert_eq!(restored.identity().public_key, identity.public_key);
    assert_eq!(
        restored.identity().signing_key_id,
        response().signing_key_id
    );
    assert!(EnrolmentRecord::load(directory.path())
        .unwrap()
        .challenge
        .is_none());
}

#[test]
fn mismatched_or_unapproved_challenges_never_change_the_credential() {
    let (directory, entry, mut candidate) = candidate();
    let original = entry.get_password().unwrap();
    let metadata = std::fs::read(directory.path().join("enrolment.json")).unwrap();
    for field in [
        "tenant",
        "repository",
        "node",
        "fingerprint",
        "request",
        "nonce",
        "state",
    ] {
        let mut altered = challenge(&candidate);
        match field {
            "tenant" => altered.tenant_id = "another-tenant".to_string(),
            "repository" => altered.repository_id = "another-repository".to_string(),
            "node" => altered.proposed_node_id = "another-node".to_string(),
            "fingerprint" => altered.public_key_fingerprint = "another-key".to_string(),
            "request" => altered.request_id.clear(),
            "nonce" => altered.nonce.clear(),
            "state" => altered.state = i32::MAX,
            _ => unreachable!(),
        }
        assert!(candidate.activation_proof(&altered).is_err());
        assert!(entry.get_password().unwrap() == original);
        assert_eq!(
            std::fs::read(directory.path().join("enrolment.json")).unwrap(),
            metadata
        );
    }
}

#[test]
fn a_candidate_cannot_be_rebound_to_another_enrollment_request() {
    let (_directory, entry, mut candidate) = candidate();
    let mut requested = challenge(&candidate);
    let proof = candidate.activation_proof(&requested).unwrap();
    let original = entry.get_password().unwrap();
    requested.request_id = "another-request".to_string();
    assert!(candidate.activation_proof(&requested).is_err());
    assert!(entry.get_password().unwrap() == original);
    assert_eq!(candidate.retry_activation_proof().unwrap(), proof);
}

#[test]
fn invalid_activation_responses_preserve_the_original_candidate() {
    for field in ["request", "state", "key", "receipt", "rejection"] {
        let (directory, entry, mut candidate) = candidate();
        candidate.activation_proof(&challenge(&candidate)).unwrap();
        let original = entry.get_password().unwrap();
        let mut result = response();
        match field {
            "request" => result.request_id = "another-request".to_string(),
            "state" => result.state = i32::MAX,
            "key" => result.signing_key_id.clear(),
            "receipt" => result.enrolment_receipt_id.clear(),
            "rejection" => result.rejection_reason = 1,
            _ => unreachable!(),
        }
        assert!(matches!(
            candidate.accept_activation(&result),
            Err(CredentialProviderError::InvalidActivation)
        ));
        assert!(entry.get_password().unwrap() == original);
        assert!(CredentialCandidate::recover_with(
            "tenant-test",
            "repo-test",
            directory.path(),
            |_| Ok(entry.clone())
        )
        .is_ok());
    }
}

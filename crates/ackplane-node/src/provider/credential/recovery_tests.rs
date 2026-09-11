use ackplane_protocol::enrollment::activation_challenge_bytes;
use ed25519_dalek::{Signature as EdSignature, VerifyingKey};

use super::{
    enrollment_tests::{candidate, challenge, response},
    CredentialCandidate, CredentialProvider, CredentialProviderError,
};
use crate::{EnrolmentRecord, NodeSigner};

#[test]
fn a_recorded_challenge_replays_the_same_proof_after_restart() {
    let (directory, entry, mut candidate) = candidate();
    let identity = candidate.identity();
    let original = candidate.activation_proof(&challenge(&candidate)).unwrap();
    drop(candidate);
    let recovered =
        CredentialCandidate::recover_with("tenant-test", "repo-test", directory.path(), |_| {
            Ok(entry.clone())
        })
        .unwrap();
    assert_eq!(recovered.retry_activation_proof().unwrap(), original);
    VerifyingKey::from_bytes(&identity.public_key)
        .unwrap()
        .verify_strict(
            &activation_challenge_bytes(
                &original.nonce,
                &original.request_id,
                &original.tenant_id,
                &original.repository_id,
                &original.proposed_node_id,
                &original.public_key_fingerprint,
            ),
            &EdSignature::from_slice(&original.signature).unwrap(),
        )
        .unwrap();
}

#[test]
fn recovery_finishes_activation_when_public_metadata_publication_was_interrupted() {
    let (directory, entry, mut candidate) = candidate();
    let identity = candidate.identity();
    candidate.activation_proof(&challenge(&candidate)).unwrap();
    let path = directory.path().join("enrolment.json");
    let original = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(candidate.accept_activation(&response()).is_err());
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(&path, original).unwrap();
    let recovered =
        CredentialProvider::recover_with("tenant-test", "repo-test", directory.path(), |_| {
            Ok(entry.clone())
        })
        .unwrap();
    assert_eq!(recovered.identity().public_key, identity.public_key);
    assert_eq!(
        recovered.activation().signing_key_id,
        response().signing_key_id
    );
    let record = EnrolmentRecord::load(directory.path()).unwrap();
    assert_eq!(
        record.activation.as_ref().unwrap().signing_key_id,
        response().signing_key_id
    );
    assert!(record.challenge.is_none());
}

#[test]
fn recovery_finishes_challenge_publication_without_changing_the_proof() {
    let (directory, entry, mut candidate) = candidate();
    let expected = challenge(&candidate);
    let path = directory.path().join("enrolment.json");
    let original = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(candidate.activation_proof(&expected).is_err());
    drop(candidate);
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(&path, original).unwrap();
    let recovered =
        CredentialCandidate::recover_with("tenant-test", "repo-test", directory.path(), |_| {
            Ok(entry.clone())
        })
        .unwrap();
    let proof = recovered.retry_activation_proof().unwrap();
    assert_eq!(proof.nonce, expected.nonce);
    assert_eq!(proof.request_id, expected.request_id);
    assert!(EnrolmentRecord::load(directory.path())
        .unwrap()
        .challenge
        .is_some());
}

#[test]
fn a_refreshed_challenge_preserves_the_request_and_survives_recovery() {
    let (directory, entry, mut candidate) = candidate();
    let mut refreshed = challenge(&candidate);
    let previous = candidate.activation_proof(&refreshed).unwrap();
    refreshed.nonce[0] ^= 1;
    let current = candidate.activation_proof(&refreshed).unwrap();
    assert_eq!(current.request_id, previous.request_id);
    assert_ne!(current.signature, previous.signature);
    drop(candidate);
    let recovered =
        CredentialCandidate::recover_with("tenant-test", "repo-test", directory.path(), |_| {
            Ok(entry.clone())
        })
        .unwrap();
    assert_eq!(recovered.retry_activation_proof().unwrap(), current);
}

#[test]
fn credential_loss_stops_enrollment_without_replacing_identity_or_metadata() {
    let (directory, entry, mut candidate) = candidate();
    candidate.activation_proof(&challenge(&candidate)).unwrap();
    let path = directory.path().join("enrolment.json");
    let original = std::fs::read(&path).unwrap();
    entry.delete_password().unwrap();
    assert!(matches!(
        candidate.retry_activation_proof(),
        Err(CredentialProviderError::Facility(_))
    ));
    assert!(matches!(
        candidate.accept_activation(&response()),
        Err(CredentialProviderError::Facility(_))
    ));
    assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

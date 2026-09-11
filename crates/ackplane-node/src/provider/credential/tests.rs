use std::sync::Arc;

use ed25519_dalek::{Signature as EdSignature, VerifyingKey};
use keyring::{mock::MockCredential, Entry};

use super::storage::MAX_CREDENTIAL_BYTES;
use super::*;
use crate::{NodeSigner, SigningBinding};

#[test]
fn candidate_provisioning_survives_restart_without_an_assigned_signing_key_id() {
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
    let identity = candidate.identity();
    drop(candidate);

    let recovered =
        CredentialCandidate::recover_with("tenant-test", "repo-test", directory.path(), |_| {
            Ok(entry.clone())
        })
        .unwrap();

    assert_eq!(recovered.identity(), identity);
    assert_eq!(identity.node_id, "node-test");
    let metadata: serde_json::Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("enrolment.json")).unwrap())
            .unwrap();
    assert!(metadata.get("signing_key_id").is_none());
    assert!(metadata.get("activation").is_none());
}

#[test]
fn oversized_binding_does_not_publish_an_unrecoverable_credential() {
    let directory = tempfile::tempdir().unwrap();
    let entry = credential();
    let mut oversized = binding();
    oversized.node_id = "n".repeat(MAX_CREDENTIAL_BYTES);

    assert!(matches!(
        CredentialProvider::provision_with(oversized, directory.path(), |_| Ok(entry.clone())),
        Err(CredentialProviderError::InvalidBinding)
    ));
    assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
    assert!(directory.path().join("ackplane-node.lock").exists());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

pub(super) fn binding() -> SigningBinding {
    SigningBinding {
        tenant_id: "tenant-test".to_string(),
        repository_id: "repo-test".to_string(),
        node_id: "node-test".to_string(),
        key_id: "key-test".to_string(),
    }
}

pub(super) fn credential() -> Arc<Entry> {
    Arc::new(Entry::new_with_credential(Box::new(
        MockCredential::default(),
    )))
}

// The short provider fingerprint was rejected by enrollment; use the wire contract's full fingerprint.
#[test]
fn candidate_fingerprint_matches_the_enrollment_protocol() {
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
    let identity = candidate.identity();

    assert_eq!(
        identity.fingerprint,
        ackplane_protocol::enrollment::public_key_fingerprint(&identity.public_key),
    );
}

#[test]
fn restart_restores_the_same_public_identity_and_signing_key() {
    let directory = tempfile::tempdir().unwrap();
    let entry = credential();
    let original =
        CredentialProvider::provision_with(binding(), directory.path(), |_| Ok(entry.clone()))
            .unwrap();
    let identity = original.identity();
    let message = b"node-restart-signature-test";
    let signature = original.sign("claim", &binding(), message).unwrap();
    drop(original);

    let restored =
        CredentialProvider::recover_with("tenant-test", "repo-test", directory.path(), |_| {
            Ok(entry.clone())
        })
        .unwrap();

    assert_eq!(restored.identity(), identity);
    assert_eq!(
        restored.sign("claim", &binding(), message).unwrap(),
        signature
    );
    VerifyingKey::from_bytes(&identity.public_key)
        .unwrap()
        .verify_strict(
            message,
            &EdSignature::from_slice(signature.as_bytes()).unwrap(),
        )
        .unwrap();
    let metadata = std::fs::read_to_string(directory.path().join("enrolment.json")).unwrap();
    let fields: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(fields["provider_scheme"], "credential-facility-software");
    assert!(
        !metadata.contains(&entry.get_password().unwrap()),
        "public metadata must not contain the stored secret"
    );
}

#[test]
fn missing_credential_refuses_recovery_without_generating_a_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let entry = credential();
    let provider =
        CredentialProvider::provision_with(binding(), directory.path(), |_| Ok(entry.clone()))
            .unwrap();
    drop(provider);
    let metadata = std::fs::read(directory.path().join("enrolment.json")).unwrap();
    entry.delete_password().unwrap();

    let result =
        CredentialProvider::recover_with("tenant-test", "repo-test", directory.path(), |_| {
            Ok(entry.clone())
        });

    assert!(matches!(result, Err(CredentialProviderError::Facility(_))));
    assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
    assert_eq!(
        std::fs::read(directory.path().join("enrolment.json")).unwrap(),
        metadata
    );
}

#[test]
fn a_replaced_or_malformed_credential_is_refused_without_modifying_it() {
    let directory = tempfile::tempdir().unwrap();
    let entry = credential();
    let provider =
        CredentialProvider::provision_with(binding(), directory.path(), |_| Ok(entry.clone()))
            .unwrap();
    drop(provider);
    for replacement in [
        "not-an-Ed25519-key".to_string(),
        serde_json::to_string(&[42_u8; 32].as_slice()).unwrap(),
    ] {
        entry.set_password(&replacement).unwrap();
        let result =
            CredentialProvider::recover_with("tenant-test", "repo-test", directory.path(), |_| {
                Ok(entry.clone())
            });
        assert!(result.is_err());
        assert!(entry.get_password().unwrap() == replacement);
    }
}

#[test]
fn a_valid_but_replaced_seed_is_refused_on_recovery_and_signing() {
    let directory = tempfile::tempdir().unwrap();
    let entry = credential();
    let provider =
        CredentialProvider::provision_with(binding(), directory.path(), |_| Ok(entry.clone()))
            .unwrap();
    let mut stored: serde_json::Value =
        serde_json::from_str(&entry.get_password().unwrap()).unwrap();
    stored["seed"] = serde_json::json!([42_u8; 32].as_slice());
    let replacement = serde_json::to_string(&stored).unwrap();
    entry.set_password(&replacement).unwrap();

    assert!(provider.sign("claim", &binding(), b"payload").is_err());
    drop(provider);
    assert!(matches!(
        CredentialProvider::recover_with("tenant-test", "repo-test", directory.path(), |_| Ok(
            entry.clone()
        )),
        Err(CredentialProviderError::IdentityMismatch)
    ));
    assert!(entry.get_password().unwrap() == replacement);
}

#[test]
fn credential_errors_and_debug_output_never_include_secret_material() {
    let directory = tempfile::tempdir().unwrap();
    let entry = credential();
    let provider =
        CredentialProvider::provision_with(binding(), directory.path(), |_| Ok(entry.clone()))
            .unwrap();
    let encoded = entry.get_password().unwrap();
    assert!(!format!("{provider:?}").contains(&encoded));
    let marker = "provider-private-error-payload";
    let mock = entry
        .get_credential()
        .downcast_ref::<MockCredential>()
        .unwrap();
    mock.set_error(keyring::Error::BadEncoding(marker.as_bytes().to_vec()));
    let error = provider.sign("claim", &binding(), b"payload").unwrap_err();
    assert!(!error.to_string().contains(marker));
    assert!(!format!("{error:?}").contains(marker));
    assert!(error.to_string().contains("credential facility"));
}

#[test]
fn failed_metadata_publication_cleans_up_only_the_new_credential() {
    let directory = tempfile::tempdir().unwrap();
    let entry = credential();
    let path = directory.path().join("enrolment.json");
    let result = CredentialProvider::provision_with(binding(), directory.path(), |_| {
        std::fs::write(&path, b"a competing enrollment record").unwrap();
        Ok(entry.clone())
    });
    assert!(matches!(
        result,
        Err(CredentialProviderError::Enrollment(_))
    ));
    assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"a competing enrollment record"
    );
    assert!(directory.path().join("ackplane-node.lock").exists());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 2);
}

#[test]
fn a_second_provider_cannot_provision_or_open_the_same_repository() {
    let directory = tempfile::tempdir().unwrap();
    let entry = credential();
    let provider =
        CredentialProvider::provision_with(binding(), directory.path(), |_| Ok(entry.clone()))
            .unwrap();

    assert!(matches!(
        CredentialProvider::provision_with(binding(), directory.path(), |_| panic!(
            "must not reach the credential store"
        )),
        Err(CredentialProviderError::Lock(_))
    ));
    assert!(matches!(
        CredentialProvider::recover_with("tenant-test", "repo-test", directory.path(), |_| panic!(
            "must not reach the credential store"
        )),
        Err(CredentialProviderError::Lock(_))
    ));
    drop(provider);
    assert!(
        CredentialProvider::provision_with(binding(), directory.path(), |_| panic!(
            "existing enrollment must not be replaced"
        ))
        .is_err()
    );
}

#[test]
fn an_existing_credential_without_metadata_is_not_overwritten() {
    let directory = tempfile::tempdir().unwrap();
    let entry = credential();
    entry.set_password("an-existing-credential").unwrap();

    assert!(matches!(
        CredentialProvider::provision_with(binding(), directory.path(), |_| Ok(entry.clone())),
        Err(CredentialProviderError::AlreadyProvisioned)
    ));
    assert_eq!(entry.get_password().unwrap(), "an-existing-credential");
    assert!(!directory.path().join("enrolment.json").exists());
}

#[test]
fn provider_loss_and_binding_mismatch_stop_signing() {
    let directory = tempfile::tempdir().unwrap();
    let entry = credential();
    let provider =
        CredentialProvider::provision_with(binding(), directory.path(), |_| Ok(entry.clone()))
            .unwrap();
    let mut wrong = binding();
    wrong.tenant_id = "another-tenant".to_string();
    assert!(matches!(
        provider.sign("claim", &wrong, b"payload"),
        Err(crate::NodeSignerError::BindingMismatch { .. })
    ));
    entry.delete_password().unwrap();
    assert!(provider.sign("claim", &binding(), b"payload").is_err());
    assert!(provider.provision_successor().is_err());
    assert!(provider
        .retire(&crate::KeyHandle::from_signing_key_id("key-test"))
        .is_err());
    assert!(provider
        .destroy(&crate::KeyHandle::from_signing_key_id("key-test"))
        .is_err());
}

#[test]
fn public_metadata_cannot_rebind_an_existing_credential_to_another_node_or_key() {
    let directory = tempfile::tempdir().unwrap();
    let entry = credential();
    let provider =
        CredentialProvider::provision_with(binding(), directory.path(), |_| Ok(entry.clone()))
            .unwrap();
    drop(provider);
    let original = crate::EnrolmentRecord::load(directory.path()).unwrap();
    let secret = entry.get_password().unwrap();
    for field in ["node", "key"] {
        let mut changed = original.clone();
        match field {
            "node" => changed.node_id = "another-node".to_string(),
            "key" => {
                changed.activation.as_mut().unwrap().signing_key_id = "another-key".to_string()
            }
            _ => unreachable!(),
        }
        std::fs::write(
            directory.path().join("enrolment.json"),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();

        let result =
            CredentialProvider::recover_with("tenant-test", "repo-test", directory.path(), |_| {
                Ok(entry.clone())
            });

        assert!(
            result.is_err(),
            "public metadata must not rebind the protected credential"
        );
        assert!(entry.get_password().unwrap() == secret);
    }
}

#[test]
fn wrong_scope_or_provider_metadata_is_refused_before_credential_access() {
    let directory = tempfile::tempdir().unwrap();
    let entry = credential();
    let provider =
        CredentialProvider::provision_with(binding(), directory.path(), |_| Ok(entry.clone()))
            .unwrap();
    drop(provider);
    assert!(CredentialProvider::recover_with(
        "another-tenant",
        "repo-test",
        directory.path(),
        |_| panic!("wrong scope must not access credentials")
    )
    .is_err());
    assert!(CredentialProvider::recover_with(
        "tenant-test",
        "another-repository",
        directory.path(),
        |_| panic!("wrong scope must not access credentials")
    )
    .is_err());
    let original = crate::EnrolmentRecord::load(directory.path()).unwrap();
    for field in ["scheme", "missing-handle", "invalid-handle"] {
        let mut changed = original.clone();
        match field {
            "scheme" => changed.provider_scheme = "unsupported-provider".to_string(),
            "missing-handle" => changed.provider_handle = None,
            "invalid-handle" => changed.provider_handle = Some("arbitrary-account".to_string()),
            _ => unreachable!(),
        }
        std::fs::write(
            directory.path().join("enrolment.json"),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        assert!(CredentialProvider::recover_with(
            "tenant-test",
            "repo-test",
            directory.path(),
            |_| panic!("invalid provider must not access credentials")
        )
        .is_err());
    }
}

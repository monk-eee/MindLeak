use std::{io::Write, path::PathBuf, process::Command};

use ed25519_dalek::{Signature as EdSignature, VerifyingKey};

use super::*;
use crate::EnrolmentRecord;

const CHILD_STATE: &str = "MINDLEAK_NODE_CREDENTIAL_RESTART_TEST";
const EXIT_WITHOUT_DROP: &str = "MINDLEAK_NODE_CREDENTIAL_EXIT_WITHOUT_DROP";
const RESTORED: &str = "restored-original-node-signature";
const TEST_NAME: &str =
    "provider::credential::platform_tests::a_persistent_provider_survives_a_real_process_restart";

#[test]
fn a_persistent_provider_survives_a_real_process_restart() {
    if std::env::var("MINDLEAK_REQUIRE_CREDENTIAL_FACILITY").as_deref() != Ok("1") {
        eprintln!("skipped: set MINDLEAK_REQUIRE_CREDENTIAL_FACILITY=1 for real credential-store validation");
        return;
    }
    let binding = SigningBinding {
        tenant_id: "credential-test-tenant".to_string(),
        repository_id: "credential-test-repository".to_string(),
        node_id: "credential-test-node".to_string(),
        key_id: "credential-test-key".to_string(),
    };
    if let Some(path) = std::env::var_os(CHILD_STATE) {
        let path = PathBuf::from(path);
        let provider =
            CredentialProvider::recover(&binding.tenant_id, &binding.repository_id, &path)
                .expect("a separate process must recover the original credential");
        let identity = provider.identity();
        let message = b"a signature from the restored node process";
        let signature = provider.sign("claim", &binding, message).unwrap();
        VerifyingKey::from_bytes(&identity.public_key)
            .unwrap()
            .verify_strict(
                message,
                &EdSignature::from_slice(signature.as_bytes()).unwrap(),
            )
            .unwrap();
        println!("{RESTORED}");
        std::io::stdout().flush().unwrap();
        if std::env::var(EXIT_WITHOUT_DROP).as_deref() == Ok("1") {
            std::process::exit(0);
        }
        return;
    }

    let directory = tempfile::tempdir().unwrap();
    let provider = CredentialProvider::provision_with(
        binding.clone(),
        directory.path(),
        CredentialStorage::entry,
    )
    .expect("the explicitly required OS credential store must accept provisioning");
    let original = provider.identity();
    drop(provider);
    let record = EnrolmentRecord::load(directory.path()).unwrap();
    assert_eq!(record.public_key, original.public_key);

    let results = [false, true, false].map(|abrupt| {
        let result = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST_NAME, "--nocapture"])
            .env(CHILD_STATE, directory.path())
            .env(EXIT_WITHOUT_DROP, if abrupt { "1" } else { "0" })
            .output();
        (abrupt, result)
    });
    let cleanup =
        CredentialStorage::entry(record.provider_handle.as_deref().unwrap()).and_then(|entry| {
            entry
                .delete_password()
                .map_err(CredentialProviderError::from)
        });
    cleanup.expect("delete only the randomly addressed test credential");

    for (abrupt, result) in results {
        let output = result.expect("the restart test child must start");
        assert!(
            output.status.success(),
            "child stdout: {}\nchild stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains(RESTORED));
        if !abrupt {
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        }
    }
    assert!(matches!(
        CredentialProvider::recover(&binding.tenant_id, &binding.repository_id, directory.path()),
        Err(CredentialProviderError::Facility(_))
    ));
}

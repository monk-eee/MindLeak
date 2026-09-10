use std::{path::PathBuf, process::Command};

use ed25519_dalek::{Signature as EdSignature, VerifyingKey};

use super::*;

const CHILD_STATE: &str = "MINDLEAK_NODE_CREDENTIAL_RESTART_TEST";
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
        return;
    }

    let directory = tempfile::tempdir().unwrap();
    let provider = CredentialProvider::provision(binding.clone(), directory.path())
        .expect("the explicitly required OS credential store must accept provisioning");
    let original = provider.identity();
    drop(provider);
    let record = EnrolmentRecord::load(directory.path()).unwrap();
    assert_eq!(record.public_key, original.public_key);

    let result = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST_NAME, "--nocapture"])
        .env(CHILD_STATE, directory.path())
        .output();
    let cleanup =
        CredentialProvider::entry(record.provider_handle.as_deref().unwrap()).and_then(|entry| {
            entry
                .delete_password()
                .map_err(CredentialProviderError::from)
        });
    cleanup.expect("delete only the randomly addressed test credential");

    let output = result.expect("the restart test child must start");
    assert!(
        output.status.success(),
        "child stdout: {}\nchild stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(matches!(
        CredentialProvider::recover(&binding.tenant_id, &binding.repository_id, directory.path()),
        Err(CredentialProviderError::Facility(_))
    ));
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
}

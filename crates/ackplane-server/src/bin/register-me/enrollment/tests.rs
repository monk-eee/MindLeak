use super::*;

#[tokio::test]
async fn blank_identity_fields_are_named_before_provider_access() {
    let directory = tempfile::tempdir().unwrap();
    let original = HashMap::from([
        (
            "provider".to_string(),
            "credential-facility-software".to_string(),
        ),
        ("tenant-id".to_string(), "tenant-test".to_string()),
        ("repo".to_string(), "repository-test".to_string()),
        ("node".to_string(), "node-test".to_string()),
        (
            "state-dir".to_string(),
            directory.path().to_string_lossy().into_owned(),
        ),
    ]);
    for name in ["tenant-id", "repo", "node"] {
        let mut flags = original.clone();
        flags.insert(name.to_string(), " ".to_string());
        assert_eq!(
            run_request(flags).await.unwrap_err(),
            format!("--{name} is required")
        );
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}

// Enrollment used to create a raw seed implicitly; require a provider before any side effect.
#[tokio::test]
async fn requesting_enrollment_requires_an_explicit_provider_before_creating_identity() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node.key");
    let flags = HashMap::from([
        ("tenant-id".to_string(), "tenant-test".to_string()),
        ("repo".to_string(), "repository-test".to_string()),
        ("node".to_string(), "node-test".to_string()),
        (
            "grpc-endpoint".to_string(),
            "http://127.0.0.1:1".to_string(),
        ),
        ("key-path".to_string(), path.to_string_lossy().into_owned()),
        (
            "state-dir".to_string(),
            directory.path().to_string_lossy().into_owned(),
        ),
    ]);
    let error = run_request(flags)
        .await
        .expect_err("provider selection is required");
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    assert!(error.contains("--provider"));
}

// Approval errors used to print the password-bearing URL into captured terminal output.
#[tokio::test]
async fn approval_errors_preserve_failure_context_without_database_credentials() {
    let password = "approval-password-must-not-be-logged";
    for (database_url, context) in [
        (
            format!("postgresql://admin:{password}@127.0.0.1:invalid/enrollment"),
            "database pool",
        ),
        (
            format!("host=127.0.0.1 password={password} port=invalid"),
            "database pool",
        ),
        (
            format!("postgresql://admin:{password}@127.0.0.1:1/enrollment?connect_timeout=1"),
            "connect",
        ),
    ] {
        let flags = HashMap::from([
            ("request-id".to_string(), "request-test".to_string()),
            ("tenant-id".to_string(), "tenant-test".to_string()),
            ("repo".to_string(), "repository-test".to_string()),
            ("fingerprint".to_string(), "fingerprint-test".to_string()),
            ("admin-database-url".to_string(), database_url.clone()),
        ]);
        let error = tokio::time::timeout(std::time::Duration::from_secs(3), run_approve(flags))
            .await
            .unwrap()
            .unwrap_err();
        assert!(!error.contains(password));
        assert!(!error.contains(&database_url));
        assert!(error.contains(context));
    }
}

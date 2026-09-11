use super::*;

#[test]
fn parse_flags_reads_flag_value_pairs() {
    let args: Vec<String> = ["--repo", "r", "--node", "n"]
        .into_iter()
        .map(str::to_string)
        .collect();
    let flags = parse_flags(&args).unwrap();
    assert_eq!(flags.get("repo").map(String::as_str), Some("r"));
    assert_eq!(flags.get("node").map(String::as_str), Some("n"));
    assert_eq!(flags.len(), 2);
}

#[test]
fn parse_flags_keeps_skip_sync_separate_from_value_flags() {
    for args in [
        ["--skip-sync", "--request-id", "request-test"],
        ["--request-id", "request-test", "--skip-sync"],
    ] {
        let flags = parse_flags(&args.map(str::to_string)).unwrap();
        assert!(flags.contains_key("skip-sync"));
        assert_eq!(
            flags.get("request-id").map(String::as_str),
            Some("request-test")
        );
        assert_eq!(flags.len(), 2);
    }
}

#[test]
fn parse_flags_refuses_a_dangling_flag_with_no_value() {
    let args: Vec<String> = ["--repo", "r", "--dangling"]
        .into_iter()
        .map(str::to_string)
        .collect();
    assert!(parse_flags(&args).is_err());
}

#[test]
fn require_reports_the_missing_flag_by_name() {
    let flags = HashMap::new();
    let error = require(&flags, "repo").expect_err("must be missing");
    assert_eq!(error, "--repo is required");
}

#[test]
fn state_directory_requires_an_explicit_absolute_path() {
    let flags = HashMap::new();
    assert!(state_directory(&flags).is_err());
    let flags = HashMap::from([("state-dir".to_string(), "relative-path".to_string())]);
    assert!(state_directory(&flags).is_err());
}

#[test]
fn state_directory_honors_an_explicit_absolute_path() {
    let directory = tempfile::tempdir().unwrap();
    let mut flags = HashMap::new();
    flags.insert(
        "state-dir".to_string(),
        directory.path().to_string_lossy().into_owned(),
    );
    assert_eq!(state_directory(&flags).unwrap(), directory.path());
}

#[test]
fn state_path_is_public_request_metadata_not_a_key_path() {
    assert_eq!(
        state_path(Path::new("state")),
        PathBuf::from("state/enrollment-request.json")
    );
}

#[test]
fn dev_tenant_token_is_stable_and_not_the_bare_name() {
    let salt = b"a-fixed-test-salt";
    let first = dev_tenant_token(salt, "demo-tenant");
    let second = dev_tenant_token(salt, "demo-tenant");
    assert_eq!(first, second);
    assert_ne!(first, "demo-tenant");
    assert_eq!(first.len(), 64);
}

#[test]
fn dev_tenant_token_differs_across_tenant_names_under_the_same_salt() {
    let salt = b"a-fixed-test-salt";
    assert_ne!(
        dev_tenant_token(salt, "tenant-a"),
        dev_tenant_token(salt, "tenant-b")
    );
}

#[test]
fn resolve_tenant_id_prefers_an_explicit_tenant_id_override() {
    let mut flags = HashMap::new();
    flags.insert("tenant-id".to_string(), "raw-token".to_string());
    // No --tenant-name/--salt-path supplied; if the override were not
    // honoured this would fail trying to require --tenant-name.
    assert_eq!(resolve_tenant_id(&flags).unwrap(), "raw-token");
}

#[test]
fn resolve_tenant_id_derives_from_name_and_salt_file() {
    let directory = tempfile::tempdir().unwrap();
    let salt_path = directory.path().join("salt.bin");
    std::fs::write(&salt_path, b"a-fixed-test-salt").expect("write salt");

    let mut flags = HashMap::new();
    flags.insert("tenant-name".to_string(), "demo-tenant".to_string());
    flags.insert(
        "salt-path".to_string(),
        salt_path.to_string_lossy().into_owned(),
    );

    let resolved = resolve_tenant_id(&flags).expect("resolves");
    assert_eq!(
        resolved,
        dev_tenant_token(b"a-fixed-test-salt", "demo-tenant")
    );

    std::fs::remove_file(&salt_path).ok();
}

#[test]
fn resolve_tenant_id_fails_without_either_form() {
    let flags = HashMap::new();
    let error = resolve_tenant_id(&flags).expect_err("must fail");
    assert_eq!(
        error,
        "either --tenant-id or --tenant-name + --salt-path is required"
    );
}

#[test]
fn raw_key_configuration_is_not_an_enrollment_provider() {
    for args in [["--key-path", "node.key"], ["--seed", "private"]] {
        assert!(parse_flags(&args.map(str::to_string)).is_err());
    }
    assert!(parse_flags(&["--repo", "first", "--repo", "second"].map(str::to_string)).is_err());
}

// A corrupt key used to be overwritten, changing an enrolled identity without consent.
#[test]
fn retired_key_option_preserves_a_corrupt_existing_seed() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("node.key");
    let corrupt = b"damaged-enrollment-key";
    std::fs::write(&path, corrupt).expect("write corrupt fixture");

    let result = parse_flags(&[
        "--key-path".to_string(),
        path.to_string_lossy().into_owned(),
    ]);
    let preserved = std::fs::read(&path).expect("read fixture") == corrupt;
    std::fs::remove_file(&path).expect("remove fixture");

    assert!(preserved, "an invalid persistent key must not be replaced");
    assert!(result.is_err());
}

// Activation must use the approved key, never silently generate a different identity.
#[tokio::test]
async fn activation_preserves_missing_corrupt_and_mismatched_approved_keys() {
    for (existing, expected_error) in [
        (None, "--key-path"),
        (Some(vec![7; 3]), "--key-path"),
        (Some(vec![8; 32]), "--key-path"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("node.key");
        let state = state_path(directory.path());
        let saved_bytes = b"saved legacy request";
        std::fs::write(&state, saved_bytes).unwrap();
        if let Some(bytes) = &existing {
            std::fs::write(&path, bytes).unwrap();
        }
        let flags = HashMap::from([
            ("request-id".to_string(), "request-test".to_string()),
            ("key-path".to_string(), path.to_string_lossy().into_owned()),
            (
                "state-dir".to_string(),
                directory.path().to_string_lossy().into_owned(),
            ),
        ]);

        let error = enrollment::run_activate(flags)
            .await
            .expect_err("must refuse the key");

        assert!(error.contains(expected_error));
        assert!(
            !error.contains("could not reach"),
            "refuse before contacting Ackplane"
        );
        match existing {
            Some(bytes) => assert_eq!(std::fs::read(&path).unwrap(), bytes),
            None => assert!(
                !path.exists(),
                "activation must not create a replacement key"
            ),
        }
        assert_eq!(std::fs::read(&state).unwrap(), saved_bytes);
    }
}

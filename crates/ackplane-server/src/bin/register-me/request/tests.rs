use super::*;

fn pending() -> SavedRequest {
    let public_key = [7; 32];
    SavedRequest::new(
        "tenant-test",
        "repository-test",
        &CandidateIdentity {
            node_id: "node-test".to_string(),
            public_key,
            fingerprint: public_key_fingerprint(&public_key),
        },
        "http://127.0.0.1:8443",
        "test node".to_string(),
        vec!["synchronize".to_string()],
    )
    .unwrap()
}

#[test]
fn pending_request_round_trips_without_private_key_or_duplicate_activation_state() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("request.json");
    let saved = pending();
    saved.save(&path).unwrap();
    assert_eq!(SavedRequest::load(&path).unwrap(), saved);
    let json: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    for field in ["seed", "private_key", "activation", "activation_nonce"] {
        assert!(json.get(field).is_none());
    }
    assert_eq!(saved.request().public_key, saved.public_key);
}

#[test]
fn a_different_request_cannot_replace_the_saved_enrollment() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("request.json");
    let saved = pending();
    saved.save(&path).unwrap();
    let original = fs::read(&path).unwrap();
    for field in [
        "tenant",
        "repository",
        "node",
        "fingerprint",
        "endpoint",
        "capabilities",
        "display",
    ] {
        let mut changed = saved.clone();
        match field {
            "tenant" => changed.tenant_id = "other".to_string(),
            "repository" => changed.repository_id = "other".to_string(),
            "node" => changed.node_id = "other".to_string(),
            "fingerprint" => changed.public_key_fingerprint = "other".to_string(),
            "endpoint" => changed.grpc_endpoint = "http://127.0.0.1:1".to_string(),
            "capabilities" => changed.requested_capabilities.push("other".to_string()),
            "display" => changed.display_name = "other".to_string(),
            _ => unreachable!(),
        }
        assert!(saved.ensure_matches(&changed).is_err());
        assert!(changed.save(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }
}

#[test]
fn repeated_request_preserves_original_timestamps() {
    let saved = pending();
    let mut repeated = saved.clone();
    repeated.created_at = "later".to_string();
    repeated.expires_at = "later".to_string();
    saved.ensure_matches(&repeated).unwrap();
    assert_eq!(saved.request_id, pending().request_id);
}

#[test]
fn incomplete_or_malformed_request_is_refused_without_changing_it() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("request.json");
    for field in [
        "request_id",
        "public_key_fingerprint",
        "created_at",
        "expires_at",
        "node_id",
    ] {
        let mut raw = serde_json::to_value(pending()).unwrap();
        raw[field] = serde_json::json!("");
        let bytes = serde_json::to_vec(&raw).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(SavedRequest::load(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn failed_request_write_preserves_the_existing_entry() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("request.json");
    fs::create_dir(&path).unwrap();
    assert!(pending().save(&path).is_err());
    assert!(path.is_dir());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn request_rejects_credentials_and_non_authority_endpoints() {
    for endpoint in [
        "http://user:password@localhost",
        "file:///tmp/state",
        "http://localhost/path",
        "http://localhost?token=private",
    ] {
        let error = validate_endpoint(endpoint).unwrap_err();
        assert!(!error.contains("password"));
        assert!(!error.contains("private"));
    }
    validate_endpoint("https://localhost:8443").unwrap();
}

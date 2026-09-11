use ackplane_client::{companion::NodeClient, TLS_CA_PATH_ENV};
use ackplane_server::{
    db_pool::{build_pool, TEST_POOL_MAX_SIZE},
    enrollment_store::{EnrollmentApproval, EnrollmentStore},
};
use rcgen::generate_simple_self_signed;
use serde_json::Value;
use tonic::transport::Identity;

use super::{
    companion_tests,
    support::{start_server, TestIdentity},
};

#[tokio::test]
async fn tls_enrollment_and_companion_restart_require_the_original_trusted_ca() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let pool = build_pool(&database_url, TEST_POOL_MAX_SIZE).unwrap();
    let directory = TestIdentity::new();
    let certificate = generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
    let wrong_certificate = generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
    let ca_path = directory.path().join("trusted-ca.pem");
    let wrong_ca_path = directory.path().join("wrong-ca.pem");
    let missing_ca_path = directory.path().join("missing-ca.pem");
    std::fs::write(&ca_path, certificate.cert.pem()).unwrap();
    std::fs::write(&wrong_ca_path, wrong_certificate.cert.pem()).unwrap();
    let (endpoint, shutdown, server) = start_server(
        &pool,
        true,
        None,
        None,
        Some(Identity::from_pem(
            certificate.cert.pem(),
            certificate.key_pair.serialize_pem(),
        )),
    )
    .await;
    let tenant = format!(
        "tls-{}",
        directory.path().file_name().unwrap().to_string_lossy()
    );
    let args = [
        "request",
        "--provider",
        "credential-facility-software",
        "--tenant-id",
        &tenant,
        "--repo",
        "repo-test",
        "--node",
        "node-test",
        "--grpc-endpoint",
        &endpoint,
    ];
    let untrusted = directory.run(&args, Some(&wrong_ca_path)).await;
    assert!(!untrusted.status.success());
    assert!(String::from_utf8_lossy(&untrusted.stderr).contains("request is saved for retry"));
    let metadata_path = directory.path().join("enrolment.json");
    let request_path = directory.path().join("enrollment-request.json");
    let metadata = std::fs::read(&metadata_path).unwrap();
    let request = std::fs::read(&request_path).unwrap();
    for ca in [None, Some(missing_ca_path.as_path())] {
        let refused = directory.run(&args, ca).await;
        assert!(!refused.status.success());
        if ca.is_some() {
            assert!(String::from_utf8_lossy(&refused.stderr).contains(TLS_CA_PATH_ENV));
        }
        assert_eq!(std::fs::read(&metadata_path).unwrap(), metadata);
        assert_eq!(std::fs::read(&request_path).unwrap(), request);
    }
    let request_count: i64 = pool
        .get()
        .await
        .unwrap()
        .query_one(
            "SELECT count(*) FROM enrollment_requests WHERE tenant_id = $1",
            &[&tenant],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        request_count, 0,
        "untrusted TLS must not submit the request"
    );
    for _attempt in 0..2 {
        let requested = directory.run(&args, Some(&ca_path)).await;
        assert!(
            requested.status.success(),
            "{}",
            String::from_utf8_lossy(&requested.stderr)
        );
        assert_eq!(std::fs::read(&metadata_path).unwrap(), metadata);
        assert_eq!(std::fs::read(&request_path).unwrap(), request);
    }

    let pending: Value = serde_json::from_slice(&request).unwrap();
    let original: Value = serde_json::from_slice(&metadata).unwrap();
    let request_id = pending["request_id"].as_str().unwrap();
    EnrollmentStore::connect(&pool)
        .await
        .unwrap()
        .approve(&EnrollmentApproval {
            request_id: request_id.into(),
            tenant_id: tenant.clone(),
            repository_id: "repo-test".into(),
            public_key_fingerprint: pending["public_key_fingerprint"].as_str().unwrap().into(),
            approved_capabilities: vec!["synchronize".into()],
            approved_by: "tls-test-admin".into(),
        })
        .await
        .unwrap();
    let activate = ["activate", "--request-id", request_id];
    let activated = directory.run(&activate, Some(&ca_path)).await;
    assert!(
        activated.status.success(),
        "{}",
        String::from_utf8_lossy(&activated.stderr)
    );
    assert!(String::from_utf8_lossy(&activated.stdout).contains("authenticated:"));
    let activated_bytes = std::fs::read(&metadata_path).unwrap();
    let activated_record: Value = serde_json::from_slice(&activated_bytes).unwrap();
    assert_eq!(activated_record["public_key"], original["public_key"]);
    assert_eq!(
        activated_record["provider_handle"],
        original["provider_handle"]
    );

    for ca in [
        None,
        Some(wrong_ca_path.as_path()),
        Some(missing_ca_path.as_path()),
    ] {
        let refused = directory.run(&activate, ca).await;
        assert!(
            !refused.status.success(),
            "saved activation must still verify TLS before sync"
        );
        assert_eq!(std::fs::read(&metadata_path).unwrap(), activated_bytes);
        assert_eq!(std::fs::read(&request_path).unwrap(), request);
    }
    let repeated = directory.run(&activate, Some(&ca_path)).await;
    assert!(
        repeated.status.success(),
        "{}",
        String::from_utf8_lossy(&repeated.stderr)
    );
    assert_eq!(std::fs::read(&metadata_path).unwrap(), activated_bytes);
    let counts = pool
        .get()
        .await
        .unwrap()
        .query_one(
            "SELECT (SELECT count(*) FROM enrollment_requests WHERE tenant_id = $1), \
             (SELECT count(*) FROM enrollment_receipts WHERE tenant_id = $1), \
             (SELECT count(*) FROM signing_keys WHERE tenant_id = $1)",
            &[&tenant],
        )
        .await
        .unwrap();
    assert_eq!(counts.get::<_, i64>(0), 1);
    assert_eq!(counts.get::<_, i64>(1), 1);
    assert_eq!(counts.get::<_, i64>(2), 1);

    let mut companion = companion_tests::start(&directory, &endpoint, Some(&ca_path)).await;
    let node = NodeClient::new(directory.path().into(), tenant.clone(), "repo-test".into());
    let identity = node.identity().await.unwrap();
    assert_eq!(identity.node_id, "node-test");
    assert_eq!(
        identity.signing_key_id,
        activated_record["activation"]["signing_key_id"]
    );
    assert!(node.status().await.unwrap().verified);
    drop(node.open_sync(0, None).await.unwrap());
    for (other_tenant, other_repository) in [
        (format!("{tenant}-other"), "repo-test"),
        (tenant.clone(), "other-repository"),
    ] {
        let other = NodeClient::new(
            directory.path().into(),
            other_tenant,
            other_repository.into(),
        );
        assert!(other.identity().await.is_err());
        assert!(other.status().await.is_err());
        assert!(other.open_sync(0, None).await.is_err());
    }
    companion.kill().await.unwrap();
    companion.wait().await.unwrap();
    assert!(node.identity().await.is_err());

    for ca in [
        None,
        Some(wrong_ca_path.as_path()),
        Some(missing_ca_path.as_path()),
    ] {
        let refused = directory.run(&["serve"], ca).await;
        assert!(
            !refused.status.success(),
            "the companion must verify TLS before serving"
        );
        assert!(!String::from_utf8_lossy(&refused.stdout).contains("node companion ready:"));
        assert_eq!(std::fs::read(&metadata_path).unwrap(), activated_bytes);
    }
    let mut companion = companion_tests::start(&directory, &endpoint, Some(&ca_path)).await;
    assert_eq!(node.identity().await.unwrap(), identity);
    assert!(node.status().await.unwrap().verified);
    drop(node.open_sync(0, None).await.unwrap());
    companion.kill().await.unwrap();
    companion.wait().await.unwrap();
    assert_eq!(std::fs::read(&metadata_path).unwrap(), activated_bytes);
    assert_eq!(std::fs::read(&request_path).unwrap(), request);
    assert!(!directory
        .path()
        .join(ackplane_client::DEFAULT_KEY_PATH)
        .exists());
    shutdown.send(()).unwrap();
    server.await.unwrap();
}

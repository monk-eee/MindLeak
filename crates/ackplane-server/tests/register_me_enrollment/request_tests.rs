use super::*;

// The old CLI saved a request only after the RPC, losing its retry identity on a dropped reply.
#[tokio::test]
async fn a_lost_request_response_replays_without_a_new_identity() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let pool = build_pool(&database_url, TEST_POOL_MAX_SIZE).unwrap();
    let dropped = Arc::new(Mutex::new(None));
    let (endpoint, shutdown, server) =
        start_server(&pool, false, None, Some(dropped.clone())).await;
    let directory = TestIdentity::new();
    let tenant = format!(
        "request-{}",
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
    let first = run_cli(directory.path(), &args).await;
    assert!(!first.status.success());
    assert!(String::from_utf8_lossy(&first.stderr).contains("request response lost after commit"));
    let request = std::fs::read(directory.path().join("enrollment-request.json")).unwrap();
    let identity = std::fs::read(directory.path().join("enrolment.json")).unwrap();
    let retried = run_cli(directory.path(), &args).await;
    assert!(
        retried.status.success(),
        "{}",
        String::from_utf8_lossy(&retried.stderr)
    );
    assert_eq!(
        std::fs::read(directory.path().join("enrollment-request.json")).unwrap(),
        request
    );
    assert_eq!(
        std::fs::read(directory.path().join("enrolment.json")).unwrap(),
        identity
    );
    let count: i64 = pool
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
    assert_eq!(count, 1);
    shutdown.send(()).unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn native_cli_request_recovers_the_same_credential_without_a_seed_file() {
    if std::env::var("MINDLEAK_REQUIRE_CREDENTIAL_FACILITY").as_deref() != Ok("1") {
        eprintln!(
            "skipped: set MINDLEAK_REQUIRE_CREDENTIAL_FACILITY=1 for native CLI verification"
        );
        return;
    }
    let directory = TestIdentity::new();
    let args = [
        "request",
        "--provider",
        "credential-facility-software",
        "--tenant-id",
        "native-cli-test",
        "--repo",
        "repo-test",
        "--node",
        "node-test",
        "--grpc-endpoint",
        "http://127.0.0.1:1",
    ];
    let first = run_cli(directory.path(), &args).await;
    assert!(!first.status.success());
    assert!(
        String::from_utf8_lossy(&first.stderr).contains("request is saved for retry"),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let metadata = std::fs::read(directory.path().join("enrolment.json")).unwrap();
    let request = std::fs::read(directory.path().join("enrollment-request.json")).unwrap();
    let second = run_cli(directory.path(), &args).await;
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("request is saved for retry"));
    assert_eq!(
        std::fs::read(directory.path().join("enrolment.json")).unwrap(),
        metadata
    );
    assert_eq!(
        std::fs::read(directory.path().join("enrollment-request.json")).unwrap(),
        request
    );
    assert!(!directory
        .path()
        .join(ackplane_client::DEFAULT_KEY_PATH)
        .exists());
    let mut changed = args.to_vec();
    changed.extend(["--display-name", "changed-name"]);
    assert!(
        String::from_utf8_lossy(&run_cli(directory.path(), &changed).await.stderr)
            .contains("differs from these parameters")
    );
    directory.remove_credential().unwrap();
    let lost = run_cli(directory.path(), &args).await;
    assert!(!lost.status.success());
    assert!(String::from_utf8_lossy(&lost.stderr).contains("credential facility"));
    assert_eq!(
        std::fs::read(directory.path().join("enrolment.json")).unwrap(),
        metadata
    );
}

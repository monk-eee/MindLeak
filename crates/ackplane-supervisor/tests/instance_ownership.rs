use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::time::timeout;

fn supervisor(state: &Path, workspace: &Path, endpoint: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ackplane-supervisor"));
    command
        .env("MINDLEAK_ACKPLANE_ENDPOINT", endpoint)
        .env("MINDLEAK_ACKPLANE_TENANT_ID", "tenant-fixture")
        .env("MINDLEAK_ACKPLANE_REPOSITORY_ID", "repository-fixture")
        .env("MINDLEAK_ACKPLANE_NODE_ID", "node-fixture")
        .env("MINDLEAK_ACKPLANE_SIGNING_KEY_ID", "key-fixture")
        .env("MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED", "07".repeat(32))
        .env_remove("MINDLEAK_ACKPLANE_TLS_CA_PATH")
        .env("ACKPLANE_SUPERVISOR_ID", "ownership-fixture")
        .env("ACKPLANE_SUPERVISOR_STATE_DIR", state)
        .env("ACKPLANE_SUPERVISOR_HEARTBEAT_SECONDS", "1")
        .env(
            "ACKPLANE_SUPERVISOR_WORKERS",
            serde_json::json!({
                "first": {
                    "command": "must-not-run-without-an-assignment",
                    "args": ["{prompt}"],
                    "working_directory": workspace,
                    "branch": "agents/ownership-fixture"
                }
            })
            .to_string(),
        )
        .env("RUST_LOG", "warn")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

// Two idle supervisors used to pass the marker check and register separate
// sessions over the same state directory and workspace. Startup must have one owner.
#[tokio::test]
async fn a_second_idle_supervisor_cannot_share_the_state_directory() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let mut first = supervisor(&state, root.path(), &endpoint).spawn().unwrap();
    let _first_connection = timeout(Duration::from_secs(10), listener.accept())
        .await
        .unwrap()
        .unwrap();
    assert!(!state.join("first.worker-run.json").exists());

    let second = supervisor(&state, root.path(), &endpoint).spawn().unwrap();
    let second_result = timeout(Duration::from_secs(5), second.wait_with_output()).await;
    first.kill().await.unwrap();

    let output = second_result
        .expect("a second supervisor must refuse before connecting")
        .unwrap();
    assert!(!output.status.success());
    let diagnostic = String::from_utf8(output.stderr).unwrap();
    assert!(
        diagnostic.contains("state directory is already in use"),
        "{diagnostic}"
    );
    assert!(!state.join("first.worker-run.json").exists());
}

#[tokio::test]
async fn killing_an_idle_owner_releases_the_directory_for_restart() {
    let root = tempfile::tempdir().unwrap();
    let state = root.path().join("state");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let mut first = supervisor(&state, root.path(), &endpoint).spawn().unwrap();
    let _first_connection = timeout(Duration::from_secs(10), listener.accept())
        .await
        .unwrap()
        .unwrap();
    first.kill().await.unwrap();
    assert!(!state.join("first.worker-run.json").exists());

    let mut restarted = supervisor(&state, root.path(), &endpoint).spawn().unwrap();
    let _restarted_connection = timeout(Duration::from_secs(10), listener.accept())
        .await
        .expect("an exited idle process must not leave a stale ownership lock")
        .unwrap();
    assert!(restarted.try_wait().unwrap().is_none());
    restarted.kill().await.unwrap();
}

#[tokio::test]
async fn independent_state_directories_keep_running_concurrently() {
    let root = tempfile::tempdir().unwrap();
    let first_workspace = root.path().join("first");
    let second_workspace = root.path().join("second");
    std::fs::create_dir_all(&first_workspace).unwrap();
    std::fs::create_dir_all(&second_workspace).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let mut first = supervisor(
        &root.path().join("first-state"),
        &first_workspace,
        &endpoint,
    )
    .spawn()
    .unwrap();
    let _first_connection = timeout(Duration::from_secs(10), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut second = supervisor(
        &root.path().join("second-state"),
        &second_workspace,
        &endpoint,
    )
    .env("ACKPLANE_SUPERVISOR_ID", "independent-fixture")
    .spawn()
    .unwrap();
    let _second_connection = timeout(Duration::from_secs(10), listener.accept())
        .await
        .unwrap()
        .unwrap();
    assert!(first.try_wait().unwrap().is_none());
    assert!(second.try_wait().unwrap().is_none());
    first.kill().await.unwrap();
    second.kill().await.unwrap();
}

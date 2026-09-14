use std::{process::Stdio, time::Instant};

use super::*;

const FIXTURE_ENV: &str = "MINDLEAK_ENROLLMENT_CLEANUP_FIXTURE";

fn fixture(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "support::credential_cleanup::tests::cleanup_process_fixture",
            "--nocapture",
        ])
        .env(FIXTURE_ENV, mode)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[test]
fn cleanup_process_fixture() {
    let Ok(mode) = std::env::var(FIXTURE_ENV) else {
        return;
    };
    match mode.as_str() {
        "success" => {}
        "failure" => panic!("deliberate cleanup failure"),
        "stalled" => std::thread::park_timeout(Duration::from_secs(2)),
        _ => panic!("unknown cleanup fixture mode"),
    }
    println!("\n{COMPLETE}");
}

// A stuck native cleanup hid enrollment failures; its child must time out and be reaped.
#[test]
fn unresponsive_cleanup_returns_before_the_native_call_finishes() {
    let started = Instant::now();
    let result = run(&mut fixture("stalled"), Duration::from_millis(100));
    assert!(
        result.is_err_and(|error| error.contains("timed out")),
        "a stalled cleanup must report its deadline, not success"
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn successful_and_failed_cleanup_children_keep_their_outcomes() {
    assert!(run(&mut fixture("success"), Duration::from_secs(10)).is_ok());
    assert!(run(&mut fixture("failure"), Duration::from_secs(10)).is_err());
}

// Selecting no cleanup test used to report success and could discard recovery metadata.
#[test]
fn successful_exit_without_running_cleanup_is_refused() {
    let mut command = fixture("success");
    command.args([
        "--skip",
        "support::credential_cleanup::tests::cleanup_process_fixture",
    ]);
    assert!(run(&mut command, Duration::from_secs(10)).is_err());
}

#[test]
fn missing_metadata_needs_no_native_credential() {
    let directory = tempfile::tempdir().unwrap();
    remove(directory.path()).unwrap();
}

#[test]
fn failed_cleanup_preserves_the_original_metadata() {
    let identity = super::super::TestIdentity::new();
    let path = identity.path().to_path_buf();
    let metadata = path.join("enrolment.json");
    std::fs::write(&metadata, b"invalid enrollment metadata").unwrap();
    assert!(std::panic::catch_unwind(|| drop(identity)).is_err());
    assert_eq!(
        std::fs::read(metadata).unwrap(),
        b"invalid enrollment metadata"
    );
    std::fs::remove_dir_all(path).unwrap();
}

use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread::{self, JoinHandle},
    time::Duration,
};

use ackplane_node::{LockError, NodeProcessLock};

const CHILD_STATE: &str = "MINDLEAK_NODE_LOCK_TEST_STATE";
const READY: &str = "node-lock-owner-ready";

struct OwnerProcess {
    child: Child,
    output: Option<JoinHandle<()>>,
}

impl OwnerProcess {
    fn start(directory: &Path) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "lock_owner_fixture", "--nocapture"])
            .env(CHILD_STATE, directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let output = thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if line.ends_with(READY) {
                    let _ = ready_tx.send(());
                }
            }
        });
        let owner = Self {
            child,
            output: Some(output),
        };
        ready_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the child must acquire ownership before the parent proceeds");
        owner
    }

    fn kill(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
}

impl Drop for OwnerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(output) = self.output.take() {
            let _ = output.join();
        }
    }
}

#[test]
fn lock_owner_fixture() {
    let Some(directory) = std::env::var_os(CHILD_STATE) else {
        return;
    };
    let _owner = NodeProcessLock::acquire(Path::new(&directory)).unwrap();
    println!("{READY}");
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_line(&mut String::new()).unwrap();
}

#[test]
fn a_live_owner_refuses_a_second_process() {
    let directory = tempfile::tempdir().unwrap();
    let _owner = OwnerProcess::start(directory.path());

    assert!(matches!(
        NodeProcessLock::acquire(directory.path()),
        Err(LockError::AlreadyLocked(_))
    ));
}

// A killed node left a marker that blocked every restart; ownership must die with its process.
#[test]
fn a_killed_owner_can_restart_without_deleting_identity_state() {
    let directory = tempfile::tempdir().unwrap();
    let identity = directory.path().join("enrolment.json");
    std::fs::write(&identity, b"existing public identity").unwrap();
    let mut original = OwnerProcess::start(directory.path());
    original.kill();

    let restarted = NodeProcessLock::acquire(directory.path())
        .expect("a dead owner's file must not prevent recovery");
    assert!(matches!(
        NodeProcessLock::acquire(directory.path()),
        Err(LockError::AlreadyLocked(_))
    ));
    drop(restarted);

    let _next_process = OwnerProcess::start(directory.path());
    assert_eq!(
        std::fs::read(&identity).unwrap(),
        b"existing public identity"
    );
}

#[test]
fn graceful_release_keeps_the_stable_lock_file_for_the_next_owner() {
    let directory = tempfile::tempdir().unwrap();
    let mut owner = OwnerProcess::start(directory.path());
    owner
        .child
        .stdin
        .take()
        .unwrap()
        .write_all(b"release\n")
        .unwrap();
    assert!(owner.child.wait().unwrap().success());

    assert!(directory.path().join("ackplane-node.lock").exists());
    let _next = NodeProcessLock::acquire(directory.path()).unwrap();
}

#[test]
fn different_repository_directories_have_independent_owners() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let _first_owner = OwnerProcess::start(first.path());
    let _second_owner = OwnerProcess::start(second.path());
}

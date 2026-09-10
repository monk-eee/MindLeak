//! ADR-0116 decisions 5 and 9: `WorkerAdapter` narrows every action to a
//! worker the adapter itself registered, and `ProcessWorkerAdapter` is the
//! reference implementation over a real, deterministic, cross-platform child
//! process (the `sleep_worker` test fixture binary in this same crate).

use ackplane_protocol::supervisor::SupervisorWorkerState;
use ackplane_supervisor::{AdapterError, ProcessWorkerAdapter, WorkerAdapter, WorkerAssignment};

fn sleep_worker() -> &'static str {
    env!("CARGO_BIN_EXE_sleep_worker")
}

fn assignment(worker_id: &str, millis: u64) -> WorkerAssignment {
    WorkerAssignment {
        worker_id: worker_id.to_string(),
        command: sleep_worker().to_string(),
        args: vec![millis.to_string()],
        working_directory: std::env::temp_dir(),
    }
}

#[test]
fn starts_and_observes_a_running_then_exited_worker() {
    let mut adapter = ProcessWorkerAdapter::new();
    adapter.start(assignment("w1", 1500)).unwrap();
    assert_eq!(
        adapter.observe("w1").unwrap(),
        SupervisorWorkerState::Started
    );
    std::thread::sleep(std::time::Duration::from_millis(2500));
    assert_eq!(
        adapter.observe("w1").unwrap(),
        SupervisorWorkerState::Completed
    );
    adapter.terminate("w1").unwrap();
}

#[test]
fn terminates_an_owned_worker_and_stops_tracking_it() {
    let mut adapter = ProcessWorkerAdapter::new();
    adapter.start(assignment("w1", 30_000)).unwrap();
    adapter.terminate("w1").unwrap();
    // A terminated worker is no longer tracked -- acting on it again is
    // refused exactly like a worker that was never registered.
    assert_eq!(
        adapter.observe("w1"),
        Err(AdapterError::UnknownWorker("w1".to_string()))
    );
}

#[test]
fn refuses_every_action_against_an_unregistered_worker() {
    let mut adapter = ProcessWorkerAdapter::new();
    assert_eq!(
        adapter.observe("ghost"),
        Err(AdapterError::UnknownWorker("ghost".to_string()))
    );
    assert_eq!(
        adapter.terminate("ghost"),
        Err(AdapterError::UnknownWorker("ghost".to_string()))
    );
}

#[test]
fn refuses_a_duplicate_worker_id() {
    let mut adapter = ProcessWorkerAdapter::new();
    adapter.start(assignment("w1", 30_000)).unwrap();
    assert_eq!(
        adapter.start(assignment("w1", 30_000)),
        Err(AdapterError::DuplicateWorker("w1".to_string()))
    );
    adapter.terminate("w1").unwrap();
}

#[test]
fn refuses_a_command_that_cannot_spawn() {
    let mut adapter = ProcessWorkerAdapter::new();
    let result = adapter.start(WorkerAssignment {
        worker_id: "w1".to_string(),
        command: "this-binary-does-not-exist-anywhere".to_string(),
        args: vec![],
        working_directory: std::env::temp_dir(),
    });
    assert!(matches!(result, Err(AdapterError::SpawnFailed(_))));
}

#[test]
fn checkpoint_pause_and_drain_are_honestly_unsupported_not_approximated() {
    let mut adapter = ProcessWorkerAdapter::new();
    adapter.start(assignment("w1", 30_000)).unwrap();
    assert_eq!(
        adapter.checkpoint("w1"),
        Err(AdapterError::Unsupported("checkpoint"))
    );
    assert_eq!(adapter.pause("w1"), Err(AdapterError::Unsupported("pause")));
    assert_eq!(adapter.drain("w1"), Err(AdapterError::Unsupported("drain")));
    adapter.terminate("w1").unwrap();
}

/// A reaped leader left its descendant running after completion and termination,
/// allowing writes to continue after the supervisor released the worker's lease.
#[cfg(unix)]
#[test]
fn an_exited_group_leader_does_not_leave_a_live_descendant() {
    use std::{
        io::{ErrorKind, Read, Write},
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut adapter = ProcessWorkerAdapter::new();
    adapter
        .start(WorkerAssignment {
            worker_id: "parent".into(),
            command: sleep_worker().into(),
            args: vec![
                "--spawn-descendant".into(),
                listener.local_addr().unwrap().to_string(),
            ],
            working_directory: std::env::temp_dir(),
        })
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                assert!(Instant::now() < deadline, "descendant never connected");
                thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("fixture accept failed: {error}"),
        }
    };
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut response = [0_u8; 1];
    stream.read_exact(&mut response).unwrap();
    assert_eq!(&response, b"R");
    while adapter.observe("parent").unwrap() == SupervisorWorkerState::Started {
        assert!(Instant::now() < deadline, "group leader did not exit");
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        adapter.observe("parent").unwrap(),
        SupervisorWorkerState::Completed
    );
    let _ = stream.write_all(b"?");
    match stream.read(&mut response) {
        Ok(0) => {}
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted
            ) => {}
        other => panic!("descendant survived completed worker cleanup: {other:?}"),
    }
    assert_eq!(
        adapter.observe("parent").unwrap(),
        SupervisorWorkerState::Completed
    );
    adapter.terminate("parent").unwrap();
}

//! ADR-0116 decisions 5 and 9: a small, runtime-neutral contract that
//! translates a typed assignment into a local worker's lifecycle, while
//! narrowly bounding every action to a worker the adapter itself started.
//! `ProcessWorkerAdapter` is the reference implementation: it owns a
//! child-process tree and never accepts an action against a worker id it did
//! not itself register.

use std::collections::HashMap;
use std::process::{Child, Command};

use ackplane_protocol::supervisor::SupervisorWorkerState;
use thiserror::Error;

/// A typed unit of work handed to a worker adapter. The adapter translates
/// this into its runtime's local invocation; it never interprets business
/// semantics, and it never runs a shell interpreter over untyped text --
/// only a declared command and its argument vector (ADR-0116 decision 5:
/// "It cannot execute arbitrary shell strings").
#[derive(Debug, Clone)]
pub struct WorkerAssignment {
    pub worker_id: String,
    pub command: String,
    pub args: Vec<String>,
}

/// A local, adapter-scoped refusal. This is distinct from the wire-level
/// `SupervisorLifecycleReason`: it never leaves the process, so it is not
/// part of the closed protocol vocabulary Ackplane records receipts against.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AdapterError {
    #[error("worker {0} is not registered with this adapter")]
    UnknownWorker(String),
    #[error("worker {0} is already registered")]
    DuplicateWorker(String),
    #[error("failed to start worker: {0}")]
    SpawnFailed(String),
    #[error("this adapter cannot honestly enforce {0} for a generic process worker")]
    Unsupported(&'static str),
}

/// A narrow, local contract every runtime adapter implements (ADR-0116
/// decision 9: "a small adapter contract to AgentD, Agency, editor agents,
/// pipeline steps, and future runtimes"). `start`/`observe`/`terminate` are
/// mandatory; `checkpoint`, `pause`, and `drain` default to an honest
/// refusal an adapter overrides only when it can genuinely enforce the
/// control -- never approximated (decisions 4 and 10).
pub trait WorkerAdapter {
    /// Starts a new worker under this adapter's ownership. Refuses a
    /// `worker_id` this adapter already owns.
    fn start(&mut self, assignment: WorkerAssignment) -> Result<(), AdapterError>;

    /// Reports the last observed lifecycle state of a worker this adapter
    /// itself registered. Refuses any other `worker_id` (decision 5).
    fn observe(&mut self, worker_id: &str) -> Result<SupervisorWorkerState, AdapterError>;

    /// Terminates and stops tracking a worker this adapter itself
    /// registered. Refuses any other `worker_id` (decision 5).
    fn terminate(&mut self, worker_id: &str) -> Result<(), AdapterError>;

    /// Checkpoints a worker this adapter owns. The default refuses honestly;
    /// an adapter overrides this only when it can genuinely enforce it.
    fn checkpoint(&mut self, worker_id: &str) -> Result<(), AdapterError> {
        let _ = worker_id;
        Err(AdapterError::Unsupported("checkpoint"))
    }

    /// Pauses a worker this adapter owns. The default refuses honestly; an
    /// adapter overrides this only when it can genuinely enforce it.
    fn pause(&mut self, worker_id: &str) -> Result<(), AdapterError> {
        let _ = worker_id;
        Err(AdapterError::Unsupported("pause"))
    }

    /// Drains a worker this adapter owns. The default refuses honestly; an
    /// adapter overrides this only when it can genuinely enforce it.
    fn drain(&mut self, worker_id: &str) -> Result<(), AdapterError> {
        let _ = worker_id;
        Err(AdapterError::Unsupported("drain"))
    }
}

/// Reference adapter: owns a set of local child processes. It starts a
/// declared command and argument vector -- never a shell string -- and every
/// method refuses to act on a `worker_id` outside `self.workers` (ADR-0116
/// decision 5's narrow worker boundary). Checkpoint, pause, and drain are
/// not honestly enforceable for a generic OS process, so this adapter uses
/// the trait's default refusal rather than approximate them.
#[derive(Default)]
pub struct ProcessWorkerAdapter {
    workers: HashMap<String, Child>,
}

impl ProcessWorkerAdapter {
    pub fn new() -> Self {
        Self::default()
    }
}

impl WorkerAdapter for ProcessWorkerAdapter {
    fn start(&mut self, assignment: WorkerAssignment) -> Result<(), AdapterError> {
        if self.workers.contains_key(&assignment.worker_id) {
            return Err(AdapterError::DuplicateWorker(assignment.worker_id));
        }
        let child = Command::new(&assignment.command)
            .args(&assignment.args)
            .spawn()
            .map_err(|error| AdapterError::SpawnFailed(error.to_string()))?;
        self.workers.insert(assignment.worker_id, child);
        Ok(())
    }

    fn observe(&mut self, worker_id: &str) -> Result<SupervisorWorkerState, AdapterError> {
        let child = self
            .workers
            .get_mut(worker_id)
            .ok_or_else(|| AdapterError::UnknownWorker(worker_id.to_string()))?;
        match child.try_wait() {
            Ok(Some(status)) if status.success() => Ok(SupervisorWorkerState::Completed),
            Ok(Some(_)) => Ok(SupervisorWorkerState::Failed),
            Ok(None) => Ok(SupervisorWorkerState::Started),
            Err(error) => Err(AdapterError::SpawnFailed(error.to_string())),
        }
    }

    fn terminate(&mut self, worker_id: &str) -> Result<(), AdapterError> {
        let mut child = self
            .workers
            .remove(worker_id)
            .ok_or_else(|| AdapterError::UnknownWorker(worker_id.to_string()))?;
        if matches!(child.try_wait(), Ok(Some(_))) {
            // Already exited, and `try_wait` reaped it on the way past.
            return Ok(());
        }
        child
            .kill()
            .map_err(|error| AdapterError::SpawnFailed(error.to_string()))?;
        // `kill` only signals. Without this the child stays a zombie on Unix,
        // because `Child`'s `Drop` does not reap either.
        child
            .wait()
            .map(|_| ())
            .map_err(|error| AdapterError::SpawnFailed(error.to_string()))
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    /// Reads `/proc` directly rather than asserting `terminate` returned `Ok`:
    /// the un-reaped case also returns `Ok`, so only the process table can
    /// tell the fix from the bug.
    #[test]
    fn terminate_reaps_the_child_rather_than_leaving_a_zombie() {
        let mut adapter = ProcessWorkerAdapter::new();
        adapter
            .start(WorkerAssignment {
                worker_id: "w1".to_string(),
                command: "/bin/sleep".to_string(),
                args: vec!["30".to_string()],
            })
            .expect("a long-running child should spawn");
        let pid = adapter.workers["w1"].id();

        adapter.terminate("w1").expect("terminate should succeed");

        // A reaped child leaves no /proc entry. An un-reaped one is still
        // listed, in state Z.
        if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            // `comm` may itself contain spaces and parentheses, so the state
            // field is read after the LAST ')'.
            let state = stat
                .rsplit(')')
                .next()
                .and_then(|rest| rest.split_whitespace().next())
                .expect("/proc/<pid>/stat carries a state field after comm");
            assert_ne!(state, "Z", "terminate left pid {pid} as a zombie: {stat}");
        }
    }
}

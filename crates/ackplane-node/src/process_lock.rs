//! A repository-scoped process lock (ADR-0100 decision 1): refuses a second
//! concurrent `ackplane-node` instance for the same repository id.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("another ackplane-node instance already holds the lock at {0}")]
    AlreadyLocked(PathBuf),
    #[error("failed to acquire process lock at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Held for the lifetime of this value; the lock file is removed on `Drop`.
/// This is a best-effort, single-host lock: it does not detect or reclaim a
/// lock left behind by a process that was killed without running its
/// destructors. That is a deliberate simplification for this first slice, not
/// an oversight — see ADR-0100 decision 1's "a repository-scoped process lock
/// refuses a second active instance" for the invariant this satisfies.
pub struct NodeProcessLock {
    path: PathBuf,
}

impl NodeProcessLock {
    /// Acquires the lock under `repository_state_dir`, writing the current
    /// process id into the lock file so an operator can identify the holder.
    pub fn acquire(repository_state_dir: &Path) -> Result<Self, LockError> {
        fs::create_dir_all(repository_state_dir).map_err(|source| LockError::Io {
            path: repository_state_dir.to_path_buf(),
            source,
        })?;
        let path = repository_state_dir.join("ackplane-node.lock");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| {
                if source.kind() == std::io::ErrorKind::AlreadyExists {
                    LockError::AlreadyLocked(path.clone())
                } else {
                    LockError::Io {
                        path: path.clone(),
                        source,
                    }
                }
            })?;
        // Best-effort diagnostic only; the file's exclusive creation above is
        // what provides the actual exclusivity guarantee.
        let _ = write!(file, "{}", std::process::id());
        Ok(Self { path })
    }
}

impl Drop for NodeProcessLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_lock_for_the_same_repository_is_refused() {
        let dir = tempfile::tempdir().unwrap();

        let first = NodeProcessLock::acquire(dir.path()).unwrap();
        let second = NodeProcessLock::acquire(dir.path());

        assert!(matches!(second, Err(LockError::AlreadyLocked(_))));
        drop(first);
    }

    #[test]
    fn the_lock_can_be_reacquired_after_release() {
        let dir = tempfile::tempdir().unwrap();

        let first = NodeProcessLock::acquire(dir.path()).unwrap();
        drop(first);

        let second = NodeProcessLock::acquire(dir.path());
        assert!(second.is_ok());
    }

    #[test]
    fn two_different_repositories_do_not_contend() {
        let dir_a = tempfile::tempdir().unwrap();
        let dir_b = tempfile::tempdir().unwrap();

        let lock_a = NodeProcessLock::acquire(dir_a.path());
        let lock_b = NodeProcessLock::acquire(dir_b.path());

        assert!(lock_a.is_ok());
        assert!(lock_b.is_ok());
    }
}

//! A repository-scoped process lock (ADR-0100 decision 1): refuses a second
//! concurrent `ackplane-node` instance for the same repository id.

use std::fs::{self, File, OpenOptions};
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

/// The kernel holds ownership for this file handle's lifetime, including
/// releasing it after a process is killed. The file itself is never removed:
/// unlinking it could let contenders lock different files at the same path.
pub struct NodeProcessLock {
    _file: File,
}

impl NodeProcessLock {
    /// Acquires the lock under `repository_state_dir`, writing the current
    /// process id for diagnostics only after ownership is obtained. An existing
    /// unlocked file is reused; its contents never authorize taking ownership.
    pub fn acquire(repository_state_dir: &Path) -> Result<Self, LockError> {
        fs::create_dir_all(repository_state_dir).map_err(|source| LockError::Io {
            path: repository_state_dir.to_path_buf(),
            source,
        })?;
        let path = repository_state_dir.join("ackplane-node.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(|source| LockError::Io {
            path: path.clone(),
            source,
        })?;
        fs2::FileExt::try_lock_exclusive(&file).map_err(|source| {
            if source.raw_os_error() == fs2::lock_contended_error().raw_os_error() {
                LockError::AlreadyLocked(path.clone())
            } else {
                LockError::Io {
                    path: path.clone(),
                    source,
                }
            }
        })?;
        file.set_len(0)
            .and_then(|()| write!(file, "{}", std::process::id()))
            .map_err(|source| LockError::Io { path, source })?;
        Ok(Self { _file: file })
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

//! Serialised, one-shot lock guarding a legacy-database migration.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;

use super::{RepositoryStorageError, RETRY_DELAY};

const MIGRATION_ATTEMPTS: usize = 500;

pub(crate) fn acquire_migration_lock(
    lock_path: &Path,
    destination: &Path,
) -> Result<Option<MigrationLock>, RepositoryStorageError> {
    for _ in 0..MIGRATION_ATTEMPTS {
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(lock_path)
        {
            Ok(mut file) => {
                writeln!(file, "{}", std::process::id())?;
                file.sync_all()?;
                return Ok(Some(MigrationLock {
                    path: lock_path.to_path_buf(),
                    _file: file,
                }));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                if destination.exists() {
                    return Ok(None);
                }
                thread::sleep(RETRY_DELAY);
            }
            // Windows only: a concurrent holder's `remove_file` (this lock's own
            // `Drop`) does not remove the directory entry atomically the way
            // Unix `unlink` does. NTFS marks the file for pending delete first,
            // during which a racing `create_new` can observe `PermissionDenied`
            // instead of the clean `AlreadyExists`/success this loop already
            // handles -- observed flaking
            // `migration_lock_retries_until_a_concurrent_holder_releases_it` in
            // a full workspace run while passing every time in isolation, the
            // signature of a timing race rather than a real assertion failure
            // (gaps.d/windows-migration-lock-retry-test-can-flake-with-permission-denied.md).
            // Retrying is bounded by the same `MIGRATION_ATTEMPTS` loop as the
            // `AlreadyExists` arm, so a genuinely unwritable directory still
            // surfaces as `MigrationBusy` rather than hanging -- it is a less
            // specific error, not a silent one. Scoped to Windows because the
            // race this exists for cannot occur on Unix, where the same retry
            // would only slow down reporting a real permission failure.
            #[cfg(windows)]
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                thread::sleep(RETRY_DELAY);
            }
            Err(error) => return Err(error.into()),
        }
    }
    Err(RepositoryStorageError::MigrationBusy(
        lock_path.to_path_buf(),
    ))
}

pub(crate) struct MigrationLock {
    path: PathBuf,
    _file: File,
}

impl Drop for MigrationLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

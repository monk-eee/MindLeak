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

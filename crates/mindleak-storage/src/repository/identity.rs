//! Repository identity: the single id every worktree of a clone resolves through.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use super::fs::git_command;
use super::{RepositoryStorageError, RETRY_DELAY};

pub(crate) const REPOSITORY_ID_KEY: &str = "mindleak.repositoryId";
const REPOSITORY_ID_MARKER: &str = "mindleak-repository-id";
const REPOSITORY_ID_HEX_LEN: usize = 32;
const BOOTSTRAP_ATTEMPTS: usize = 200;

pub(crate) fn repository_id(
    workspace: &Path,
    common_dir: &Path,
) -> Result<String, RepositoryStorageError> {
    if let Some(configured) = read_repository_id(workspace)? {
        return validate_repository_id(&configured);
    }

    let marker_path = common_dir.join(REPOSITORY_ID_MARKER);
    let candidate = generate_repository_id(common_dir);
    let selected = match OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&marker_path)
    {
        Ok(mut marker) => {
            marker.write_all(candidate.as_bytes())?;
            marker.write_all(b"\n")?;
            marker.sync_all()?;
            candidate
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            read_repository_id_marker(&marker_path)?
        }
        Err(error) => return Err(error.into()),
    };

    match write_repository_id(workspace, &selected) {
        Ok(()) => Ok(selected),
        Err(error) => {
            for _ in 0..BOOTSTRAP_ATTEMPTS {
                if read_repository_id(workspace)?.as_deref() == Some(selected.as_str()) {
                    return Ok(selected);
                }
                thread::sleep(RETRY_DELAY);
            }
            Err(error)
        }
    }
}

fn read_repository_id(workspace: &Path) -> Result<Option<String>, RepositoryStorageError> {
    let output = git_command()
        .args(["config", "--local", "--get", REPOSITORY_ID_KEY])
        .current_dir(workspace)
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!value.is_empty()).then_some(value))
}

fn write_repository_id(
    workspace: &Path,
    repository_id: &str,
) -> Result<(), RepositoryStorageError> {
    let output = git_command()
        .args(["config", "--local", REPOSITORY_ID_KEY, repository_id])
        .current_dir(workspace)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(RepositoryStorageError::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

pub(crate) fn read_repository_id_marker(path: &Path) -> Result<String, RepositoryStorageError> {
    let mut last_invalid = None;
    for _ in 0..BOOTSTRAP_ATTEMPTS {
        match fs::read_to_string(path) {
            Ok(value) if !value.trim().is_empty() => {
                match validate_repository_id(value.trim()) {
                    Ok(repository_id) => return Ok(repository_id),
                    Err(RepositoryStorageError::InvalidRepositoryId(value)) => {
                        // The winning process owns the new file but may still be
                        // writing its contents. Retry before treating a stable,
                        // malformed marker as corruption.
                        last_invalid = Some(value);
                        thread::sleep(RETRY_DELAY);
                    }
                    Err(error) => return Err(error),
                }
            }
            Ok(_) => thread::sleep(RETRY_DELAY),
            Err(error) if error.kind() == io::ErrorKind::NotFound => thread::sleep(RETRY_DELAY),
            Err(error) => return Err(error.into()),
        }
    }
    match last_invalid {
        Some(value) => Err(RepositoryStorageError::InvalidRepositoryId(value)),
        None => Err(RepositoryStorageError::RepositoryIdBusy(path.to_path_buf())),
    }
}

fn validate_repository_id(value: &str) -> Result<String, RepositoryStorageError> {
    if value.len() == REPOSITORY_ID_HEX_LEN
        && value
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
    {
        Ok(value.to_string())
    } else {
        Err(RepositoryStorageError::InvalidRepositoryId(
            value.to_string(),
        ))
    }
}

fn generate_repository_id(common_dir: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(common_dir.to_string_lossy().as_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes(),
    );
    hasher.finalize()[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

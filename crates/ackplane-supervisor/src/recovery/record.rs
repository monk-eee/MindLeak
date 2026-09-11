use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use ackplane_protocol::supervisor::{
    SupervisorRegistration, SupervisorSession, SupervisorWorkerState,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::RecoveryError;
use crate::config::SupervisorConfig;

pub(crate) const MAX_MARKER_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunRecord {
    pub version: u32,
    pub slot: String,
    pub registration: SupervisorRegistration,
    pub session: SupervisorSession,
    pub task_id: String,
    pub directive_id: String,
    pub directive_digest: Vec<u8>,
    pub packet_id: String,
    pub working_directory: PathBuf,
    pub branch: String,
    pub inbox: PathBuf,
    pub outbox: PathBuf,
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn checked_file(path: &Path, directory: &Path) -> Result<PathBuf, RecoveryError> {
    let metadata = path.symlink_metadata()?;
    if !metadata.file_type().is_file() {
        return Err(RecoveryError::Refused(
            "run evidence must be a regular file, not a symlink".into(),
        ));
    }
    let canonical = path.canonicalize()?;
    if canonical.parent() != Some(directory) {
        return Err(RecoveryError::Refused(
            "run evidence escapes its state directory".into(),
        ));
    }
    Ok(canonical)
}

pub(crate) fn read_marker(path: &Path, directory: &Path) -> Result<Vec<u8>, RecoveryError> {
    let path = checked_file(path, directory)?;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_MARKER_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_MARKER_BYTES {
        return Err(RecoveryError::Refused("run marker exceeds 64 KiB".into()));
    }
    Ok(bytes)
}

impl RunRecord {
    pub(crate) fn load(
        config: &SupervisorConfig,
        slot: &str,
    ) -> Result<(Self, Vec<u8>, PathBuf), RecoveryError> {
        if slot.is_empty()
            || slot.len() > 64
            || !slot
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(RecoveryError::Refused("invalid worker slot".into()));
        }
        let command = config
            .workers
            .get(slot)
            .ok_or_else(|| RecoveryError::Refused("worker slot is not configured".into()))?;
        let directory = config.state_dir.canonicalize()?;
        let path = directory.join(format!("{slot}.worker-run.json"));
        let bytes = read_marker(&path, &directory)?;
        let record = Self::parse(config, slot, command, &directory, &bytes)?;
        Ok((record, bytes, path))
    }

    pub(crate) fn load_archived(
        config: &SupervisorConfig,
        slot: &str,
        run_id: &str,
    ) -> Result<(Self, Vec<u8>, PathBuf), RecoveryError> {
        let command = config
            .workers
            .get(slot)
            .filter(|_| {
                slot.len() <= 64
                    && slot
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            })
            .ok_or_else(|| RecoveryError::Refused("worker slot is not configured".into()))?;
        if !Self::matches_run_id(config, slot, run_id) {
            return Err(RecoveryError::Refused(
                "invalid original run identity".into(),
            ));
        }
        let directory = config.state_dir.canonicalize()?;
        let outbox = checked_file(&directory.join(format!("{run_id}.outbox.db")), &directory)?;
        let bytes = crate::SupervisorOutbox::archived_run_marker(&outbox)?;
        let record = Self::parse(config, slot, command, &directory, &bytes)?;
        if record.registration.supervisor_id != run_id {
            return Err(RecoveryError::Refused(
                "archived record belongs to another run".into(),
            ));
        }
        Ok((
            record,
            bytes,
            directory.join(format!("{slot}.worker-run.json")),
        ))
    }

    fn matches_run_id(config: &SupervisorConfig, slot: &str, run_id: &str) -> bool {
        run_id.len() <= 160
            && run_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            && run_id.starts_with(&format!("{}-{slot}-", config.supervisor_id))
    }

    fn parse(
        config: &SupervisorConfig,
        slot: &str,
        command: &crate::WorkerCommand,
        directory: &Path,
        bytes: &[u8],
    ) -> Result<Self, RecoveryError> {
        let record: Self = serde_json::from_slice(bytes)?;
        let supervisor_id = &record.session.supervisor_id;
        if record.version != 1
            || record.slot != slot
            || !Self::matches_run_id(config, slot, supervisor_id)
            || record.registration.supervisor_id != *supervisor_id
            || record.session.session_id != format!("{supervisor_id}:session")
            || record.session.worker_id != format!("{supervisor_id}:worker")
            || record.session.state != SupervisorWorkerState::Started
            || record.registration.identity.tenant_id != config.node.tenant_id
            || record.registration.identity.repository_id != config.node.repository_id
            || record.task_id.trim().is_empty()
            || record.directive_id.trim().is_empty()
            || record.packet_id.trim().is_empty()
            || record.directive_digest.len() != 32
            || record.working_directory != command.working_directory.canonicalize()?
            || record.branch != command.branch
        {
            return Err(RecoveryError::Refused(
                "run marker does not match the configured worker identity or workspace".into(),
            ));
        }
        record
            .registration
            .validate()
            .map_err(|error| RecoveryError::Refused(error.to_string()))?;
        record
            .session
            .validate()
            .map_err(|error| RecoveryError::Refused(error.to_string()))?;
        for (recorded, suffix) in [(&record.inbox, "inbox"), (&record.outbox, "outbox")] {
            let expected = directory.join(format!("{supervisor_id}.{suffix}.db"));
            if checked_file(&expected, directory)? != *recorded {
                return Err(RecoveryError::Refused(
                    "run marker queue path does not match its derived location".into(),
                ));
            }
        }
        Ok(record)
    }
}

use std::{fs, time::Duration};

use ackplane_client::companion::wire::{Claim, Operation, SupervisorScope};
use ackplane_protocol::v1;

use super::{
    inspect,
    record::{checked_file, read_marker, RunRecord},
    RecoveryError, RecoveryPreview,
};
use crate::{
    config::SupervisorConfig,
    daemon::{frames::registration_frame, resend_pending},
    outbox::{RecoveryAttempt, RecoveryResult},
    reconcile, Reconciliation, SupervisorOutbox,
};

pub async fn confirm(
    config: &SupervisorConfig,
    slot: &str,
    run_id: &str,
    confirmation_digest: &str,
    reason: &str,
) -> Result<RecoveryPreview, RecoveryError> {
    if reason.trim().is_empty() || reason.len() > 1024 {
        return Err(RecoveryError::Refused(
            "an operator reason of 1-1024 bytes is required".into(),
        ));
    }
    let directory = config.state_dir.canonicalize()?;
    checked_file(&directory.join("ownership.db"), &directory)?;
    let _guard = crate::storage::claim_state_directory(&directory)?;
    let (record, bytes, path, marker_present) =
        match config.worker_run_path(slot).symlink_metadata() {
            Ok(_) => {
                let (record, bytes, path) = RunRecord::load(config, slot)?;
                (record, bytes, path, true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let (record, bytes, path) = RunRecord::load_archived(config, slot, run_id)?;
                (record, bytes, path, false)
            }
            Err(error) => return Err(error.into()),
        };
    let preview = super::inspect::inspect_record(&record, &bytes)?;
    if preview.run_id != run_id {
        return Err(RecoveryError::Refused(
            "run or evidence changed since inspection; inspect again".into(),
        ));
    }
    if !preview.stopped {
        return Err(RecoveryError::Refused(
            "the original worker has no positive stop record; no process will be signalled".into(),
        ));
    }
    let outbox = SupervisorOutbox::open_read_only(
        &record.outbox,
        record.registration.clone(),
        record.session.clone(),
    )?;
    let attempt = RecoveryAttempt {
        marker_digest: &preview.marker_digest,
        confirmation_digest,
        reason,
    };
    if let Some(completion) = outbox.completed_recovery(&attempt)? {
        if completion.result_digest != preview.confirmation_digest
            || completion.server_position != preview.last_enqueued
            || preview.pending_frames != 0
        {
            return Err(RecoveryError::Refused(
                "completed cleanup evidence changed".into(),
            ));
        }
        if marker_present {
            remove_matching_marker(&path, &directory, &bytes)?;
        }
        return Ok(preview);
    }
    if !marker_present {
        return Err(RecoveryError::Refused(
            "run marker is absent without a matching completed recovery".into(),
        ));
    }
    if preview.confirmation_digest != confirmation_digest {
        return Err(RecoveryError::Refused(
            "run or evidence changed since inspection; inspect again".into(),
        ));
    }
    let outbox = SupervisorOutbox::open(
        &record.outbox,
        record.registration.clone(),
        record.session.clone(),
    )?;
    outbox.record_recovery_event(&attempt, None)?;
    let released = tokio::time::timeout(Duration::from_secs(30), async {
        let identity = config.node.identity().await.map_err(Box::new)?;
        if identity.node_id != record.registration.identity.node_id {
            return Err(RecoveryError::Refused(
                "companion identity does not own the original run".into(),
            ));
        }
        let positions = outbox.positions()?;
        let mut connection = config
            .node
            .open_sync(
                positions.acknowledged,
                Some(SupervisorScope {
                    supervisor_id: record.session.supervisor_id.clone(),
                    session_id: record.session.session_id.clone(),
                    worker_id: record.session.worker_id.clone(),
                }),
            )
            .await
            .map_err(Box::new)?;
        let registration = connection
            .exchange_supervisor_frame(registration_frame(&record.registration))
            .await
            .map_err(Box::new)?;
        if registration.supervisor_id != record.registration.supervisor_id {
            return Err(RecoveryError::Refused(
                "registration receipt names another supervisor".into(),
            ));
        }
        let accepted = registration.accepted_outbox_sequence.ok_or_else(|| {
            RecoveryError::Refused("server omitted its independent receipt position".into())
        })?;
        if matches!(
            reconcile(positions, accepted),
            Reconciliation::IncompleteEvidence { .. }
        ) {
            return Err(RecoveryError::Refused(
                "server receipt position is outside the recoverable interval".into(),
            ));
        }
        while !outbox.pending(1)?.is_empty() {
            if resend_pending(&outbox, &mut connection).await?.is_some() {
                return Err(RecoveryError::Refused(
                    "receipt delivery disconnected; inspect before retrying".into(),
                ));
            }
        }
        let release = config
            .node
            .protobuf::<v1::ClaimReleaseResult>(Operation::Claim(Claim::Release {
                task_id: record.task_id.clone(),
                owner_id: record.session.session_id.clone(),
            }))
            .await
            .map_err(Box::new)?;
        Ok::<bool, RecoveryError>(release.released)
    })
    .await
    .map_err(|_| {
        RecoveryError::Refused("cleanup deadline exceeded; evidence and marker retained".into())
    })??;
    let mut result = inspect(config, slot)?;
    outbox.record_recovery_event(
        &attempt,
        Some(&RecoveryResult {
            result_digest: result.confirmation_digest.clone(),
            server_position: result.acknowledged,
            lease_released: released,
        }),
    )?;
    result.cleanup_recorded = true;
    remove_matching_marker(&path, &directory, &bytes)?;
    Ok(result)
}

fn remove_matching_marker(
    path: &std::path::Path,
    directory: &std::path::Path,
    bytes: &[u8],
) -> Result<(), RecoveryError> {
    if read_marker(path, directory)? != bytes {
        return Err(RecoveryError::Refused(
            "run marker changed during cleanup".into(),
        ));
    }
    fs::remove_file(path)?;
    Ok(())
}

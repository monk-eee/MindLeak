use ackplane_protocol::{context_packet::ContextPacketUseReceipt, v1};
use prost::Message;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior};

use super::{
    record::{digest, RunRecord},
    RecoveryError, RecoveryPreview,
};
use crate::{config::SupervisorConfig, SupervisorOutbox};

pub fn inspect(config: &SupervisorConfig, slot: &str) -> Result<RecoveryPreview, RecoveryError> {
    let (record, bytes, _) = RunRecord::load(config, slot)?;
    inspect_record(&record, &bytes)
}

pub(super) fn inspect_record(
    record: &RunRecord,
    bytes: &[u8],
) -> Result<RecoveryPreview, RecoveryError> {
    let outbox = SupervisorOutbox::open_read_only(
        &record.outbox,
        record.registration.clone(),
        record.session.clone(),
    )?;
    let evidence = outbox.recovery_snapshot()?;
    let inbox = Connection::open_with_flags(&record.inbox, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let _snapshot = Transaction::new_unchecked(&inbox, TransactionBehavior::Deferred)?;
    if !crate::storage::ensure_supervisor_identity(
        &inbox,
        &record.registration.identity,
        &record.session.supervisor_id,
        &record.session,
    )? {
        return Err(RecoveryError::Refused(
            "inbox identity does not match the run".into(),
        ));
    }
    let effect_size: Option<u64> = inbox
        .query_row(
            "SELECT length(receipt) FROM directive_effects WHERE directive_id = ?1",
            [&record.directive_id],
            |row| row.get(0),
        )
        .optional()?;
    if effect_size.is_some_and(|size| size > 1024 * 1024) {
        return Err(RecoveryError::Refused(
            "original effect exceeds the 1 MiB inspection limit".into(),
        ));
    }
    let effect_bytes = crate::storage::load_effect(&inbox, &record.directive_id)?
        .ok_or_else(|| RecoveryError::Refused("original directive effect is missing".into()))?;
    let effect = v1::DirectiveReceipt::decode(effect_bytes.as_slice()).map_err(|_| {
        RecoveryError::Refused("original directive effect cannot be decoded".into())
    })?;
    if effect.tenant_id != record.registration.identity.tenant_id
        || effect.repository_id != record.registration.identity.repository_id
        || effect.node_id != record.registration.identity.node_id
        || effect.agent_session_id != record.session.session_id
        || effect.directive_id != record.directive_id
        || effect.payload_digest != record.directive_digest
        || !effect.evidence_refs.contains(&record.packet_id)
        || !matches!(
            v1::DirectiveReceiptStatus::try_from(effect.status),
            Ok(v1::DirectiveReceiptStatus::Applied | v1::DirectiveReceiptStatus::Failed)
        )
    {
        return Err(RecoveryError::Refused(
            "original directive effect does not match this run".into(),
        ));
    }
    let mut terminal = None;
    for queued in &evidence.frames {
        match &queued.frame.frame {
            Some(v1::node_frame::Frame::SupervisorLifecycleReceipt(receipt)) => {
                if receipt.supervisor_id != record.session.supervisor_id
                    || receipt.session_id != record.session.session_id
                    || receipt.worker_id != record.session.worker_id
                    || receipt.outbox_sequence != Some(queued.sequence)
                    || receipt.idempotency_key.is_empty()
                {
                    return Err(RecoveryError::Refused(
                        "lifecycle receipt does not match the original worker".into(),
                    ));
                }
                time::OffsetDateTime::parse(
                    &receipt.occurred_at,
                    &time::format_description::well_known::Rfc3339,
                )
                .map_err(|_| RecoveryError::Refused("invalid lifecycle timestamp".into()))?;
                match v1::SupervisorWorkerState::try_from(receipt.state) {
                    Ok(
                        v1::SupervisorWorkerState::Completed
                        | v1::SupervisorWorkerState::Failed
                        | v1::SupervisorWorkerState::Terminated,
                    ) if terminal.is_none() => terminal = Some(queued.sequence),
                    Ok(v1::SupervisorWorkerState::Started) if terminal.is_none() => {}
                    _ => {
                        return Err(RecoveryError::Refused(
                            "contradictory or unsupported lifecycle history".into(),
                        ))
                    }
                }
            }
            Some(v1::node_frame::Frame::ContextPacketUseReport(receipt)) => {
                let usage: ContextPacketUseReceipt = serde_json::from_slice(&receipt.receipt_json)?;
                if receipt.outbox_sequence != Some(queued.sequence)
                    || usage.packet_id != record.packet_id
                    || usage.scope.task_id != record.task_id
                    || usage.scope.agent_session_id != record.session.session_id
                    || usage.scope.tenant_id != record.registration.identity.tenant_id
                    || usage.scope.repository_id != record.registration.identity.repository_id
                    || terminal.is_some()
                {
                    return Err(RecoveryError::Refused(
                        "context use does not match the stopped run".into(),
                    ));
                }
            }
            Some(v1::node_frame::Frame::DirectiveReceipt(receipt))
                if receipt.agent_session_id == record.session.session_id
                    && receipt.tenant_id == record.registration.identity.tenant_id
                    && receipt.repository_id == record.registration.identity.repository_id
                    && receipt.node_id == record.registration.identity.node_id
                    && receipt.outbox_sequence == Some(queued.sequence) => {}
            _ => {
                return Err(RecoveryError::Refused(
                    "unsupported or mismatched pending recovery frame".into(),
                ))
            }
        }
    }
    let stopped = if let Some((original, receipt)) = &evidence.stop {
        if original.as_slice() != bytes || terminal != Some(receipt.sequence) {
            return Err(RecoveryError::Refused(
                "stop provenance is not bound to this exact run marker".into(),
            ));
        }
        true
    } else {
        false
    };
    let marker_digest = digest(bytes);
    let confirmation_digest = digest(&serde_json::to_vec(&(
        &bytes,
        effect_bytes,
        evidence.positions.acknowledged,
        evidence.positions.last_enqueued,
        evidence
            .frames
            .iter()
            .map(|queued| (queued.sequence, queued.frame.encode_to_vec()))
            .collect::<Vec<_>>(),
        evidence
            .stop
            .as_ref()
            .map(|(marker, frame)| (marker, frame.sequence, frame.frame.encode_to_vec())),
    ))?);
    Ok(RecoveryPreview {
        slot: record.slot.clone(),
        run_id: record.registration.supervisor_id.clone(),
        identity: record.registration.identity.clone(),
        session_id: record.session.session_id.clone(),
        worker_id: record.session.worker_id.clone(),
        task_id: record.task_id.clone(),
        working_directory: record.working_directory.clone(),
        branch: record.branch.clone(),
        cleanup_recorded: evidence.completed.as_deref() == Some(&marker_digest),
        marker_digest,
        confirmation_digest,
        stopped,
        acknowledged: evidence.positions.acknowledged,
        last_enqueued: evidence.positions.last_enqueued,
        pending_frames: evidence.pending_frames,
    })
}

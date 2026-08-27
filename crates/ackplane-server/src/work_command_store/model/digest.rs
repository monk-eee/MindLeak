use super::{
    NewWorkCommand, NewWorkCommandReceipt, WorkCommand, WorkCommandKind, WorkCommandOutcome,
    WorkCommandReceipt, WorkCommandStoreError,
};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_postgres::Row;

pub(in crate::work_command_store) fn request_digest(
    request: &NewWorkCommand,
) -> Result<Vec<u8>, WorkCommandStoreError> {
    let mut hasher = Sha256::new();
    append_bytes(&mut hasher, b"mindleak.ackplane.work-command.request.v1");
    for value in [
        request.tenant_id.as_bytes(),
        request.repository_id.as_bytes(),
        request.schema_version.as_bytes(),
        request.issuing_principal_id.as_bytes(),
        request.rationale.as_bytes(),
        request.idempotency_key.as_bytes(),
    ] {
        append_bytes(&mut hasher, value);
    }
    hasher.update(request.kind.as_i16().to_be_bytes());
    append_optional_bytes(&mut hasher, request.task_id.as_deref());
    append_optional_bytes(&mut hasher, request.delegation_id.as_deref());
    append_optional_bytes(&mut hasher, request.confirmation_id.as_deref());
    append_identifiers(&mut hasher, &request.policy_refs);
    match request.expected_task_version {
        Some(version) => {
            hasher.update([1]);
            hasher.update(version.to_be_bytes());
        }
        None => hasher.update([0]),
    }
    append_timestamp(&mut hasher, request.expires_at)?;
    append_bytes(&mut hasher, &request.payload_digest);
    Ok(hasher.finalize().to_vec())
}

/// A lost response must replay the same command rather than create a second
/// one, so Ackplane derives its opaque id from the scoped idempotency identity.
/// Changed content under that identity still conflicts through `request_digest`.
pub(in crate::work_command_store) fn assigned_command_id(request: &NewWorkCommand) -> String {
    let mut hasher = Sha256::new();
    append_bytes(&mut hasher, b"mindleak.ackplane.work-command.id.v1");
    for value in [
        request.tenant_id.as_bytes(),
        request.repository_id.as_bytes(),
        request.issuing_principal_id.as_bytes(),
        request.idempotency_key.as_bytes(),
    ] {
        append_bytes(&mut hasher, value);
    }
    let hex = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("work-command:{hex}")
}

pub(in crate::work_command_store) fn receipt_digest(
    receipt: &NewWorkCommandReceipt,
) -> Result<Vec<u8>, WorkCommandStoreError> {
    let mut hasher = Sha256::new();
    append_bytes(&mut hasher, b"mindleak.ackplane.work-command.receipt.v1");
    for value in [
        receipt.tenant_id.as_bytes(),
        receipt.repository_id.as_bytes(),
        receipt.command_id.as_bytes(),
        receipt.receipt_id.as_bytes(),
        receipt.reason.as_bytes(),
    ] {
        append_bytes(&mut hasher, value);
    }
    hasher.update(receipt.outcome.as_i16().to_be_bytes());
    append_identifiers(&mut hasher, &receipt.evidence_refs);
    append_timestamp(&mut hasher, receipt.occurred_at)?;
    Ok(hasher.finalize().to_vec())
}

pub(in crate::work_command_store) fn command_from_row(
    row: &Row,
) -> Result<WorkCommand, WorkCommandStoreError> {
    Ok(WorkCommand {
        tenant_id: row.get("tenant_id"),
        repository_id: row.get("repository_id"),
        command_id: row.get("command_id"),
        kind: WorkCommandKind::from_i16(row.get("command_kind"))?,
        schema_version: row.get("schema_version"),
        task_id: row.get("task_id"),
        issuing_principal_id: row.get("issuing_principal_id"),
        delegation_id: row.get("delegation_id"),
        policy_refs: row.get("policy_refs"),
        rationale: row.get("rationale"),
        expected_task_version: row.get("expected_task_version"),
        confirmation_id: row.get("confirmation_id"),
        expires_at: row.get("expires_at"),
        idempotency_key: row.get("idempotency_key"),
        request_digest: row.get("request_digest"),
        payload_digest: row.get("payload_digest"),
        directive_id: row.get("directive_id"),
        recorded_at: row.get("recorded_at"),
    })
}

pub(in crate::work_command_store) fn receipt_from_row(
    row: &Row,
) -> Result<WorkCommandReceipt, WorkCommandStoreError> {
    Ok(WorkCommandReceipt {
        tenant_id: row.get("tenant_id"),
        repository_id: row.get("repository_id"),
        command_id: row.get("command_id"),
        receipt_id: row.get("receipt_id"),
        outcome: WorkCommandOutcome::from_i16(row.get("outcome"))?,
        reason: row.get("reason"),
        evidence_refs: row.get("evidence_refs"),
        receipt_digest: row.get("receipt_digest"),
        occurred_at: row.get("occurred_at"),
        recorded_at: row.get("recorded_at"),
    })
}

pub(in crate::work_command_store) fn append_bytes(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

pub(in crate::work_command_store) fn append_optional_bytes(
    hasher: &mut Sha256,
    value: Option<&str>,
) {
    match value {
        Some(value) => {
            hasher.update([1]);
            append_bytes(hasher, value.as_bytes());
        }
        None => hasher.update([0]),
    }
}

fn append_identifiers(hasher: &mut Sha256, values: &[String]) {
    hasher.update((values.len() as u64).to_be_bytes());
    for value in values {
        append_bytes(hasher, value.as_bytes());
    }
}

pub(in crate::work_command_store) fn append_timestamp(
    hasher: &mut Sha256,
    timestamp: SystemTime,
) -> Result<(), WorkCommandStoreError> {
    let duration = timestamp
        .duration_since(UNIX_EPOCH)
        .map_err(|_| WorkCommandStoreError::InvalidTimestamp)?;
    hasher.update(duration.as_secs().to_be_bytes());
    hasher.update(duration.subsec_nanos().to_be_bytes());
    Ok(())
}

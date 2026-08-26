//! Bounded value objects for ADR-0119 decision 6's Recovery inspection
//! reports: read-only findings against one identified Snapshot artifact,
//! never a mutation of production authority. Unlike a Snapshot or purge
//! request, an inspection is not deduplicated by idempotency key -- it is
//! always safe to repeat, and decision 6 calls for durable reports (plural),
//! so each call appends a new immutable record rather than replaying one.

use std::time::SystemTime;

use sha2::{Digest, Sha256};
use tokio_postgres::Row;

use super::model::{append_bytes, append_timestamp, hex_id, require_identifier};
use super::AdministrationStoreError;

const MAX_REASON_BYTES: usize = 4_096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRecoveryInspection {
    pub request_id: String,
    pub requested_by: String,
    pub integrity_verified: bool,
    pub decryption_verified: bool,
    pub archive_valid: bool,
    pub archive_entry_count: Option<i64>,
    pub reason: String,
    pub occurred_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryInspection {
    pub inspection_id: String,
    pub request_id: String,
    pub requested_by: String,
    pub integrity_verified: bool,
    pub decryption_verified: bool,
    pub archive_valid: bool,
    pub archive_entry_count: Option<i64>,
    pub reason: String,
    pub occurred_at: SystemTime,
    pub recorded_at: SystemTime,
}

pub(super) fn validate(
    inspection: &NewRecoveryInspection,
    now: SystemTime,
) -> Result<(), AdministrationStoreError> {
    require_identifier("request_id", &inspection.request_id)?;
    require_identifier("requested_by", &inspection.requested_by)?;
    if inspection.reason.len() > MAX_REASON_BYTES {
        return Err(AdministrationStoreError::InvalidReason);
    }
    if inspection.occurred_at > now {
        return Err(AdministrationStoreError::InvalidReceiptTime);
    }
    Ok(())
}

/// Random, not deterministic: two inspections of the same artifact are
/// legitimately distinct records, not replays of one another, so there is no
/// identity to derive an id from the way a request or receipt has.
pub(super) fn assigned_inspection_id(
    inspection: &NewRecoveryInspection,
) -> Result<String, AdministrationStoreError> {
    let mut random = [0_u8; 16];
    getrandom::getrandom(&mut random).map_err(|_| AdministrationStoreError::InvalidTimestamp)?;
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.recovery-inspection.id.v1",
    );
    append_bytes(&mut hasher, inspection.request_id.as_bytes());
    append_timestamp(&mut hasher, inspection.occurred_at)?;
    hasher.update(random);
    Ok(hex_id("administration-recovery-inspection", hasher))
}

pub(super) fn from_row(row: &Row) -> Result<RecoveryInspection, AdministrationStoreError> {
    Ok(RecoveryInspection {
        inspection_id: row.get("inspection_id"),
        request_id: row.get("request_id"),
        requested_by: row.get("requested_by"),
        integrity_verified: row.get("integrity_verified"),
        decryption_verified: row.get("decryption_verified"),
        archive_valid: row.get("archive_valid"),
        archive_entry_count: row.get("archive_entry_count"),
        reason: row.get("reason"),
        occurred_at: row.get("occurred_at"),
        recorded_at: row.get("recorded_at"),
    })
}

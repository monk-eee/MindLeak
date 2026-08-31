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

const DIGEST_BYTES: usize = 32;

/// ADR-0145 decision 1-2: a durable, append-only record of one real restore
/// drill against an isolated, ephemeral target. Unlike an inspection (a
/// format check that never opens a database), a rehearsal proves the archive
/// actually restores against this deployment's current schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRecoveryRehearsal {
    pub request_id: String,
    pub requested_by: String,
    pub manifest_digest: Vec<u8>,
    pub restore_duration_ms: i64,
    pub migration_version_matched: bool,
    pub archive_table_count: Option<i64>,
    pub restored_table_count: Option<i64>,
    pub restored_row_count: Option<i64>,
    pub passed: bool,
    pub reason: String,
    pub occurred_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryRehearsal {
    pub rehearsal_id: String,
    pub request_id: String,
    pub requested_by: String,
    pub manifest_digest: Vec<u8>,
    pub restore_duration_ms: i64,
    pub migration_version_matched: bool,
    pub archive_table_count: Option<i64>,
    pub restored_table_count: Option<i64>,
    pub restored_row_count: Option<i64>,
    pub passed: bool,
    pub reason: String,
    pub occurred_at: SystemTime,
    pub recorded_at: SystemTime,
}

pub(super) fn validate_rehearsal(
    rehearsal: &NewRecoveryRehearsal,
    now: SystemTime,
) -> Result<(), AdministrationStoreError> {
    require_identifier("request_id", &rehearsal.request_id)?;
    require_identifier("requested_by", &rehearsal.requested_by)?;
    if rehearsal.manifest_digest.len() != DIGEST_BYTES {
        return Err(AdministrationStoreError::InvalidManifestDigest);
    }
    if rehearsal.reason.len() > MAX_REASON_BYTES {
        return Err(AdministrationStoreError::InvalidReason);
    }
    if rehearsal.occurred_at > now {
        return Err(AdministrationStoreError::InvalidReceiptTime);
    }
    Ok(())
}

/// Random, not deterministic: two rehearsals of the same artifact are
/// legitimately distinct events, not replays of one another -- mirroring
/// `assigned_inspection_id`'s own reasoning exactly.
pub(super) fn assigned_rehearsal_id(
    rehearsal: &NewRecoveryRehearsal,
) -> Result<String, AdministrationStoreError> {
    let mut random = [0_u8; 16];
    getrandom::getrandom(&mut random).map_err(|_| AdministrationStoreError::InvalidTimestamp)?;
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.recovery-rehearsal.id.v1",
    );
    append_bytes(&mut hasher, rehearsal.request_id.as_bytes());
    append_bytes(&mut hasher, &rehearsal.manifest_digest);
    append_timestamp(&mut hasher, rehearsal.occurred_at)?;
    hasher.update(random);
    Ok(hex_id("administration-recovery-rehearsal", hasher))
}

pub(super) fn from_rehearsal_row(row: &Row) -> Result<RecoveryRehearsal, AdministrationStoreError> {
    Ok(RecoveryRehearsal {
        rehearsal_id: row.get("rehearsal_id"),
        request_id: row.get("request_id"),
        requested_by: row.get("requested_by"),
        manifest_digest: row.get("manifest_digest"),
        restore_duration_ms: row.get("restore_duration_ms"),
        migration_version_matched: row.get("migration_version_matched"),
        archive_table_count: row.get("archive_table_count"),
        restored_table_count: row.get("restored_table_count"),
        restored_row_count: row.get("restored_row_count"),
        passed: row.get("passed"),
        reason: row.get("reason"),
        occurred_at: row.get("occurred_at"),
        recorded_at: row.get("recorded_at"),
    })
}

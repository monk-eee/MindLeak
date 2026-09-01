//! Bounded value objects and canonical ids for ADR-0145 decision 7's
//! production recovery-execution receipt: what actually happened when a
//! previously confirmed request's restore ran (or was refused, or failed).
//!
//! Distinct from `recovery_execution_model::RecoveryConfirmation`, which
//! records only that a second, distinct enrolled key *authorized* a request
//! -- never that anything ran. Conflating the two would let a merely
//! authorized request read as an executed one.

use std::time::SystemTime;

use sha2::{Digest, Sha256};
use tokio_postgres::Row;

use super::model::{append_bytes, append_timestamp, hex_id, require_identifier};
use super::AdministrationStoreError;

const MAX_REASON_BYTES: usize = 4_096;
const DIGEST_BYTES: usize = 32;

/// This receipt's closed outcome vocabulary (ADR-0145 decision 7), following
/// `PurgeReceipt`'s `Expired`-vs-`Refused` distinction: a request refused for
/// a reason discovered at execution time (stale rehearsal, an
/// unattested deployment) is not the same fact as a `pg_restore` that
/// actually ran and failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryExecutionOutcome {
    Succeeded,
    Failed,
    Refused,
}

impl RecoveryExecutionOutcome {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::Succeeded => 1,
            Self::Failed => 2,
            Self::Refused => 3,
        }
    }

    fn from_i16(value: i16) -> Result<Self, AdministrationStoreError> {
        match value {
            1 => Ok(Self::Succeeded),
            2 => Ok(Self::Failed),
            3 => Ok(Self::Refused),
            other => Err(AdministrationStoreError::UnknownOutcome { value: other }),
        }
    }
}

/// What executing a confirmed recovery request durably records. Every field
/// but `reason` is a fact this store already holds on the request/
/// confirmation it executes -- restated here (never joined at read time) so
/// the receipt is a self-contained provenance record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRecoveryExecutionReceipt {
    pub request_id: String,
    pub tenant_id: String,
    /// The pre-restore safety Snapshot's own digest -- the "old" state.
    pub old_manifest_digest: Vec<u8>,
    /// The restored artifact's own digest -- the "new" state.
    pub new_manifest_digest: Vec<u8>,
    pub rehearsal_id: String,
    pub previewing_node_id: String,
    pub previewing_public_key_fingerprint: String,
    pub confirming_node_id: String,
    pub confirming_public_key_fingerprint: String,
    pub outcome: RecoveryExecutionOutcome,
    pub reason: String,
    pub occurred_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryExecutionReceipt {
    pub receipt_id: String,
    pub request_id: String,
    pub tenant_id: String,
    pub old_manifest_digest: Vec<u8>,
    pub new_manifest_digest: Vec<u8>,
    pub rehearsal_id: String,
    pub previewing_node_id: String,
    pub previewing_public_key_fingerprint: String,
    pub confirming_node_id: String,
    pub confirming_public_key_fingerprint: String,
    pub outcome: RecoveryExecutionOutcome,
    pub reason: String,
    pub occurred_at: SystemTime,
    pub recorded_at: SystemTime,
}

pub(super) fn validate_receipt(
    receipt: &NewRecoveryExecutionReceipt,
    now: SystemTime,
) -> Result<(), AdministrationStoreError> {
    require_identifier("request_id", &receipt.request_id)?;
    require_identifier("tenant_id", &receipt.tenant_id)?;
    require_identifier("rehearsal_id", &receipt.rehearsal_id)?;
    require_identifier("previewing_node_id", &receipt.previewing_node_id)?;
    require_identifier(
        "previewing_public_key_fingerprint",
        &receipt.previewing_public_key_fingerprint,
    )?;
    require_identifier("confirming_node_id", &receipt.confirming_node_id)?;
    require_identifier(
        "confirming_public_key_fingerprint",
        &receipt.confirming_public_key_fingerprint,
    )?;
    if receipt.old_manifest_digest.len() != DIGEST_BYTES
        || receipt.new_manifest_digest.len() != DIGEST_BYTES
    {
        return Err(AdministrationStoreError::InvalidManifestDigest);
    }
    if receipt.reason.len() > MAX_REASON_BYTES {
        return Err(AdministrationStoreError::InvalidReason);
    }
    if receipt.occurred_at > now {
        return Err(AdministrationStoreError::InvalidReceiptTime);
    }
    Ok(())
}

/// Random, not deterministic -- mirroring `RecoveryRehearsal`'s own id
/// assignment: one receipt per request is already enforced by the table's
/// `UNIQUE (request_id)`, so there is no idempotency key here to derive from.
pub(super) fn assigned_receipt_id(
    receipt: &NewRecoveryExecutionReceipt,
) -> Result<String, AdministrationStoreError> {
    let mut random = [0_u8; 16];
    getrandom::getrandom(&mut random).map_err(|_| AdministrationStoreError::InvalidTimestamp)?;
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.recovery-execution-receipt.id.v1",
    );
    append_bytes(&mut hasher, receipt.request_id.as_bytes());
    append_timestamp(&mut hasher, receipt.occurred_at)?;
    hasher.update(random);
    Ok(hex_id("administration-recovery-execution-receipt", hasher))
}

pub(super) fn receipt_from_row(
    row: &Row,
) -> Result<RecoveryExecutionReceipt, AdministrationStoreError> {
    Ok(RecoveryExecutionReceipt {
        receipt_id: row.get("receipt_id"),
        request_id: row.get("request_id"),
        tenant_id: row.get("tenant_id"),
        old_manifest_digest: row.get("old_manifest_digest"),
        new_manifest_digest: row.get("new_manifest_digest"),
        rehearsal_id: row.get("rehearsal_id"),
        previewing_node_id: row.get("previewing_node_id"),
        previewing_public_key_fingerprint: row.get("previewing_public_key_fingerprint"),
        confirming_node_id: row.get("confirming_node_id"),
        confirming_public_key_fingerprint: row.get("confirming_public_key_fingerprint"),
        outcome: RecoveryExecutionOutcome::from_i16(row.get("outcome"))?,
        reason: row.get("reason"),
        occurred_at: row.get("occurred_at"),
        recorded_at: row.get("recorded_at"),
    })
}

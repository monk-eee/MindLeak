//! Bounded value objects and canonical digests for ADR-0119 decisions 1, 7,
//! and 9's two-phase (preview, then confirm) Lifecycle purge workflow.
//! Deliberately one closed data category today (`TelemetryEvents`): adding a
//! second means adding a variant and its own scoped delete here, never a
//! free-text table/column name accepted from a caller.

use std::time::SystemTime;

use sha2::{Digest, Sha256};
use tokio_postgres::Row;

use super::model::{append_bytes, append_timestamp, hex_id, require_identifier};
use super::AdministrationStoreError;

const MAX_REASON_BYTES: usize = 4_096;
/// A confirmation window longer than this would let a stale, unreviewed
/// preview stay executable indefinitely; bounded the same way
/// `MAX_POLICY_LIFETIME_SECONDS` bounds a policy's lifetime.
pub const MAX_CONFIRMATION_WINDOW: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

/// ADR-0119 decision 1's closed purge data-category vocabulary. Telemetry
/// events are the first category: bounded diagnostic history, not part of
/// core coordination correctness, already timestamped for age-based
/// retention (see `crates/ackplane-server/src/telemetry_store.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PurgeDataCategory {
    TelemetryEvents,
}

impl PurgeDataCategory {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::TelemetryEvents => 1,
        }
    }

    fn from_i16(value: i16) -> Result<Self, AdministrationStoreError> {
        match value {
            1 => Ok(Self::TelemetryEvents),
            other => Err(AdministrationStoreError::UnknownDataCategory { value: other }),
        }
    }
}

/// A caller's request for a purge preview: everything decision 7 requires
/// naming, before anything is deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurgePreviewRequest {
    pub policy_id: String,
    pub requested_by: String,
    pub requesting_node_id: String,
    pub requesting_public_key_fingerprint: String,
    pub tenant_id: String,
    pub repository_id: String,
    pub data_category: PurgeDataCategory,
    pub older_than: SystemTime,
    pub confirmation_window: std::time::Duration,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurgeRequest {
    pub request_id: String,
    pub policy_id: String,
    pub requested_by: String,
    pub requesting_node_id: Option<String>,
    pub requesting_public_key_fingerprint: Option<String>,
    pub tenant_id: String,
    pub repository_id: String,
    pub data_category: PurgeDataCategory,
    pub older_than: SystemTime,
    pub preview_row_count: i64,
    pub confirmation_expires_at: SystemTime,
    pub idempotency_key: String,
    pub requested_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurgeRequestOutcome {
    pub request: PurgeRequest,
    pub idempotent_replay: bool,
}

/// ADR-0119 decision 7: the receipt names what happened, never the erased
/// payload. `Expired` is distinct from `Refused` -- a confirmation that
/// arrived too late is not the same fact as one a revoked policy blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurgeOutcome {
    Succeeded,
    Failed,
    Refused,
    Expired,
}

impl PurgeOutcome {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::Succeeded => 1,
            Self::Failed => 2,
            Self::Refused => 3,
            Self::Expired => 4,
        }
    }

    fn from_i16(value: i16) -> Result<Self, AdministrationStoreError> {
        match value {
            1 => Ok(Self::Succeeded),
            2 => Ok(Self::Failed),
            3 => Ok(Self::Refused),
            4 => Ok(Self::Expired),
            other => Err(AdministrationStoreError::UnknownOutcome { value: other }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPurgeReceipt {
    pub request_id: String,
    pub outcome: PurgeOutcome,
    pub reason: String,
    pub rows_deleted: Option<i64>,
    pub occurred_at: SystemTime,
    /// The enrolled key that authenticated confirmation of this request.
    pub confirming_signing_key_id: Option<String>,
    /// The enrolled node bound to the confirming signing key.
    pub confirming_node_id: Option<String>,
    /// The public-key material fingerprint proving the confirmer differs from
    /// the requester even after a key-id rotation.
    pub confirming_public_key_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurgeReceipt {
    pub receipt_id: String,
    pub request_id: String,
    pub outcome: PurgeOutcome,
    pub reason: String,
    pub rows_deleted: Option<i64>,
    pub occurred_at: SystemTime,
    pub recorded_at: SystemTime,
    pub confirming_signing_key_id: Option<String>,
    pub confirming_node_id: Option<String>,
    pub confirming_public_key_fingerprint: Option<String>,
}

pub(super) fn validate_preview_request(
    request: &PurgePreviewRequest,
) -> Result<(), AdministrationStoreError> {
    require_identifier("requested_by", &request.requested_by)?;
    require_identifier("requesting_node_id", &request.requesting_node_id)?;
    require_identifier(
        "requesting_public_key_fingerprint",
        &request.requesting_public_key_fingerprint,
    )?;
    require_identifier("tenant_id", &request.tenant_id)?;
    require_identifier("repository_id", &request.repository_id)?;
    require_identifier("idempotency_key", &request.idempotency_key)?;
    if request.confirmation_window.is_zero()
        || request.confirmation_window > MAX_CONFIRMATION_WINDOW
    {
        return Err(AdministrationStoreError::InvalidConfirmationWindow);
    }
    Ok(())
}

pub(super) fn validate_receipt(
    receipt: &NewPurgeReceipt,
    now: SystemTime,
) -> Result<(), AdministrationStoreError> {
    if receipt.reason.len() > MAX_REASON_BYTES {
        return Err(AdministrationStoreError::InvalidReason);
    }
    if receipt.occurred_at > now {
        return Err(AdministrationStoreError::InvalidReceiptTime);
    }
    match (
        receipt.confirming_signing_key_id.as_deref(),
        receipt.confirming_node_id.as_deref(),
        receipt.confirming_public_key_fingerprint.as_deref(),
    ) {
        (Some(signing_key_id), Some(node_id), Some(fingerprint)) => {
            require_identifier("confirming_signing_key_id", signing_key_id)?;
            require_identifier("confirming_node_id", node_id)?;
            require_identifier("confirming_public_key_fingerprint", fingerprint)?;
        }
        (None, None, None) => {}
        _ => return Err(AdministrationStoreError::IncompleteConfirmationPrincipal),
    }
    Ok(())
}

pub(super) fn assigned_request_id(request: &PurgePreviewRequest) -> String {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.purge-request.id.v1",
    );
    append_bytes(&mut hasher, request.requested_by.as_bytes());
    append_bytes(&mut hasher, request.requesting_node_id.as_bytes());
    append_bytes(
        &mut hasher,
        request.requesting_public_key_fingerprint.as_bytes(),
    );
    append_bytes(&mut hasher, request.idempotency_key.as_bytes());
    hex_id("administration-purge-request", hasher)
}

pub(super) fn preview_request_digest(
    request: &PurgePreviewRequest,
) -> Result<Vec<u8>, AdministrationStoreError> {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.purge-request.v1",
    );
    append_bytes(&mut hasher, request.policy_id.as_bytes());
    append_bytes(&mut hasher, request.requested_by.as_bytes());
    append_bytes(&mut hasher, request.requesting_node_id.as_bytes());
    append_bytes(
        &mut hasher,
        request.requesting_public_key_fingerprint.as_bytes(),
    );
    append_bytes(&mut hasher, request.tenant_id.as_bytes());
    append_bytes(&mut hasher, request.repository_id.as_bytes());
    hasher.update(request.data_category.as_i16().to_be_bytes());
    append_timestamp(&mut hasher, request.older_than)?;
    append_bytes(&mut hasher, request.idempotency_key.as_bytes());
    Ok(hasher.finalize().to_vec())
}

pub(super) fn assigned_receipt_id(receipt: &NewPurgeReceipt) -> String {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.purge-receipt.id.v1",
    );
    append_bytes(&mut hasher, receipt.request_id.as_bytes());
    hex_id("administration-purge-receipt", hasher)
}

pub(super) fn receipt_digest(
    receipt: &NewPurgeReceipt,
) -> Result<Vec<u8>, AdministrationStoreError> {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.purge-receipt.v1",
    );
    append_bytes(&mut hasher, receipt.request_id.as_bytes());
    hasher.update(receipt.outcome.as_i16().to_be_bytes());
    append_bytes(&mut hasher, receipt.reason.as_bytes());
    match receipt.rows_deleted {
        Some(rows) => {
            hasher.update([1]);
            hasher.update(rows.to_be_bytes());
        }
        None => hasher.update([0]),
    }
    match &receipt.confirming_signing_key_id {
        Some(signing_key_id) => {
            hasher.update([1]);
            append_bytes(&mut hasher, signing_key_id.as_bytes());
        }
        None => hasher.update([0]),
    }
    match &receipt.confirming_node_id {
        Some(node_id) => {
            hasher.update([1]);
            append_bytes(&mut hasher, node_id.as_bytes());
        }
        None => hasher.update([0]),
    }
    match &receipt.confirming_public_key_fingerprint {
        Some(fingerprint) => {
            hasher.update([1]);
            append_bytes(&mut hasher, fingerprint.as_bytes());
        }
        None => hasher.update([0]),
    }
    append_timestamp(&mut hasher, receipt.occurred_at)?;
    Ok(hasher.finalize().to_vec())
}

pub(super) fn request_from_row(row: &Row) -> Result<PurgeRequest, AdministrationStoreError> {
    Ok(PurgeRequest {
        request_id: row.get("request_id"),
        policy_id: row.get("policy_id"),
        requested_by: row.get("requested_by"),
        requesting_node_id: row.get("requesting_node_id"),
        requesting_public_key_fingerprint: row.get("requesting_public_key_fingerprint"),
        tenant_id: row.get("tenant_id"),
        repository_id: row.get("repository_id"),
        data_category: PurgeDataCategory::from_i16(row.get("data_category"))?,
        older_than: row.get("older_than"),
        preview_row_count: row.get("preview_row_count"),
        confirmation_expires_at: row.get("confirmation_expires_at"),
        idempotency_key: row.get("idempotency_key"),
        requested_at: row.get("requested_at"),
    })
}

pub(super) fn receipt_from_row(row: &Row) -> Result<PurgeReceipt, AdministrationStoreError> {
    Ok(PurgeReceipt {
        receipt_id: row.get("receipt_id"),
        request_id: row.get("request_id"),
        outcome: PurgeOutcome::from_i16(row.get("outcome"))?,
        reason: row.get("reason"),
        rows_deleted: row.get("rows_deleted"),
        occurred_at: row.get("occurred_at"),
        recorded_at: row.get("recorded_at"),
        confirming_signing_key_id: row.get("confirming_signing_key_id"),
        confirming_node_id: row.get("confirming_node_id"),
        confirming_public_key_fingerprint: row.get("confirming_public_key_fingerprint"),
    })
}

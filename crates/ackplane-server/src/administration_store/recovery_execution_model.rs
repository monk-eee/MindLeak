//! Bounded value objects and canonical digests for ADR-0145 decision 4-5's
//! production recovery execution preview/confirmation workflow.
//!
//! Deliberately mirrors `purge_model.rs`'s `PurgePreviewRequest`/
//! `PurgeReceipt` shape: ADR-0145 decision 4 reuses ADR-0134's dual-signing-
//! key preview/confirmation pattern verbatim, scoped to Recovery, rather than
//! inventing a second competing authorization mechanism. `MAX_CONFIRMATION_WINDOW`
//! itself is imported from `purge_model`, not redefined here -- the same
//! bound, not a parallel copy of it.
//!
//! This slice's confirmation is an *authorization* outcome only
//! (`RecoveryConfirmationOutcome`): `Confirmed` records that a second,
//! distinct enrolled key authorized this exact request, never that
//! `pg_restore` ran against production. Slice 4 adds its own, later
//! `RecoveryExecutionReceipt` (ADR-0145 decision 7) that consumes a
//! `Confirmed` row here as a precondition -- the two must stay distinct
//! records, or a merely-authorized request would read as an executed one.

use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};
use tokio_postgres::Row;

use super::model::{append_bytes, append_timestamp, hex_id, require_identifier};
pub use super::purge_model::MAX_CONFIRMATION_WINDOW;
use super::AdministrationStoreError;

const MAX_REASON_BYTES: usize = 4_096;
const DIGEST_BYTES: usize = 32;

/// A caller's request for a recovery-execution preview: the explicit impact
/// plan ADR-0145 decision 5 requires naming, before anything may be
/// confirmed. The safety-snapshot fields are already-captured facts by the
/// time this reaches the store -- triggering that Snapshot is an
/// orchestration step the Bridge route performs first (this store never
/// executes a snapshot itself, matching every other record in this module);
/// its failure fails the preview by never reaching this call at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryExecutionPreviewRequest {
    pub policy_id: String,
    pub requested_by: String,
    /// The Bridge tenant that made this request, so disclosure of the
    /// request/confirmation is bounded to it -- the recovery-execution
    /// *scope* itself stays always platform-wide (decision 6).
    pub tenant_id: String,
    pub requesting_node_id: String,
    pub requesting_public_key_fingerprint: String,
    /// The Snapshot request naming the artifact being restored.
    pub artifact_request_id: String,
    /// Caller-declared digest of that artifact, cross-checked at preview
    /// time against the artifact's own recorded Snapshot receipt.
    pub manifest_digest: Vec<u8>,
    /// The fresh platform Snapshot's own receipt id, captured as part of
    /// preview construction (decision 5's "one before" safety point).
    pub safety_snapshot_receipt_id: String,
    pub safety_snapshot_digest: Vec<u8>,
    /// The passing rehearsal report this preview relies on.
    pub rehearsal_id: String,
    pub confirmation_window: Duration,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryExecutionRequest {
    pub request_id: String,
    pub policy_id: String,
    pub requested_by: String,
    pub tenant_id: String,
    pub requesting_node_id: String,
    pub requesting_public_key_fingerprint: String,
    pub artifact_request_id: String,
    pub manifest_digest: Vec<u8>,
    pub safety_snapshot_receipt_id: String,
    pub safety_snapshot_digest: Vec<u8>,
    pub rehearsal_id: String,
    pub confirmation_expires_at: SystemTime,
    pub idempotency_key: String,
    pub requested_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryExecutionRequestOutcome {
    pub request: RecoveryExecutionRequest,
    pub idempotent_replay: bool,
}

/// This slice's closed confirmation outcome vocabulary -- authorization
/// only. `Succeeded`/`Failed` belong to slice 4's own execution receipt,
/// which records whether `pg_restore` itself succeeded; nothing here ever
/// claims that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryConfirmationOutcome {
    Confirmed,
    Refused,
    Expired,
}

impl RecoveryConfirmationOutcome {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::Confirmed => 1,
            Self::Refused => 2,
            Self::Expired => 3,
        }
    }

    fn from_i16(value: i16) -> Result<Self, AdministrationStoreError> {
        match value {
            1 => Ok(Self::Confirmed),
            2 => Ok(Self::Refused),
            3 => Ok(Self::Expired),
            other => Err(AdministrationStoreError::UnknownOutcome { value: other }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRecoveryConfirmation {
    pub request_id: String,
    pub outcome: RecoveryConfirmationOutcome,
    pub reason: String,
    pub occurred_at: SystemTime,
    pub confirming_signing_key_id: Option<String>,
    pub confirming_node_id: Option<String>,
    pub confirming_public_key_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryConfirmation {
    pub confirmation_id: String,
    pub request_id: String,
    pub outcome: RecoveryConfirmationOutcome,
    pub reason: String,
    pub occurred_at: SystemTime,
    pub recorded_at: SystemTime,
    pub confirming_signing_key_id: Option<String>,
    pub confirming_node_id: Option<String>,
    pub confirming_public_key_fingerprint: Option<String>,
}

pub(super) fn validate_preview_request(
    request: &RecoveryExecutionPreviewRequest,
) -> Result<(), AdministrationStoreError> {
    require_identifier("requested_by", &request.requested_by)?;
    require_identifier("tenant_id", &request.tenant_id)?;
    require_identifier("requesting_node_id", &request.requesting_node_id)?;
    require_identifier(
        "requesting_public_key_fingerprint",
        &request.requesting_public_key_fingerprint,
    )?;
    require_identifier("artifact_request_id", &request.artifact_request_id)?;
    require_identifier(
        "safety_snapshot_receipt_id",
        &request.safety_snapshot_receipt_id,
    )?;
    require_identifier("rehearsal_id", &request.rehearsal_id)?;
    require_identifier("idempotency_key", &request.idempotency_key)?;
    if request.manifest_digest.len() != DIGEST_BYTES {
        return Err(AdministrationStoreError::InvalidManifestDigest);
    }
    if request.safety_snapshot_digest.len() != DIGEST_BYTES {
        return Err(AdministrationStoreError::InvalidManifestDigest);
    }
    if request.confirmation_window.is_zero()
        || request.confirmation_window > MAX_CONFIRMATION_WINDOW
    {
        return Err(AdministrationStoreError::InvalidConfirmationWindow);
    }
    Ok(())
}

pub(super) fn validate_confirmation(
    confirmation: &NewRecoveryConfirmation,
    now: SystemTime,
) -> Result<(), AdministrationStoreError> {
    if confirmation.reason.len() > MAX_REASON_BYTES {
        return Err(AdministrationStoreError::InvalidReason);
    }
    if confirmation.occurred_at > now {
        return Err(AdministrationStoreError::InvalidReceiptTime);
    }
    match (
        confirmation.confirming_signing_key_id.as_deref(),
        confirmation.confirming_node_id.as_deref(),
        confirmation.confirming_public_key_fingerprint.as_deref(),
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

pub(super) fn assigned_request_id(request: &RecoveryExecutionPreviewRequest) -> String {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.recovery-execution-request.id.v1",
    );
    append_bytes(&mut hasher, request.requested_by.as_bytes());
    append_bytes(&mut hasher, request.requesting_node_id.as_bytes());
    append_bytes(
        &mut hasher,
        request.requesting_public_key_fingerprint.as_bytes(),
    );
    append_bytes(&mut hasher, request.idempotency_key.as_bytes());
    hex_id("administration-recovery-execution-request", hasher)
}

pub(super) fn preview_request_digest(
    request: &RecoveryExecutionPreviewRequest,
) -> Result<Vec<u8>, AdministrationStoreError> {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.recovery-execution-request.v1",
    );
    append_bytes(&mut hasher, request.policy_id.as_bytes());
    append_bytes(&mut hasher, request.requested_by.as_bytes());
    append_bytes(&mut hasher, request.tenant_id.as_bytes());
    append_bytes(&mut hasher, request.requesting_node_id.as_bytes());
    append_bytes(
        &mut hasher,
        request.requesting_public_key_fingerprint.as_bytes(),
    );
    append_bytes(&mut hasher, request.artifact_request_id.as_bytes());
    append_bytes(&mut hasher, &request.manifest_digest);
    append_bytes(&mut hasher, request.safety_snapshot_receipt_id.as_bytes());
    append_bytes(&mut hasher, &request.safety_snapshot_digest);
    append_bytes(&mut hasher, request.rehearsal_id.as_bytes());
    append_bytes(&mut hasher, request.idempotency_key.as_bytes());
    Ok(hasher.finalize().to_vec())
}

pub(super) fn assigned_confirmation_id(confirmation: &NewRecoveryConfirmation) -> String {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.recovery-execution-confirmation.id.v1",
    );
    append_bytes(&mut hasher, confirmation.request_id.as_bytes());
    hex_id("administration-recovery-execution-confirmation", hasher)
}

pub(super) fn confirmation_digest(
    confirmation: &NewRecoveryConfirmation,
) -> Result<Vec<u8>, AdministrationStoreError> {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.recovery-execution-confirmation.v1",
    );
    append_bytes(&mut hasher, confirmation.request_id.as_bytes());
    hasher.update(confirmation.outcome.as_i16().to_be_bytes());
    append_bytes(&mut hasher, confirmation.reason.as_bytes());
    match &confirmation.confirming_signing_key_id {
        Some(signing_key_id) => {
            hasher.update([1]);
            append_bytes(&mut hasher, signing_key_id.as_bytes());
        }
        None => hasher.update([0]),
    }
    match &confirmation.confirming_node_id {
        Some(node_id) => {
            hasher.update([1]);
            append_bytes(&mut hasher, node_id.as_bytes());
        }
        None => hasher.update([0]),
    }
    match &confirmation.confirming_public_key_fingerprint {
        Some(fingerprint) => {
            hasher.update([1]);
            append_bytes(&mut hasher, fingerprint.as_bytes());
        }
        None => hasher.update([0]),
    }
    append_timestamp(&mut hasher, confirmation.occurred_at)?;
    Ok(hasher.finalize().to_vec())
}

pub(super) fn request_from_row(
    row: &Row,
) -> Result<RecoveryExecutionRequest, AdministrationStoreError> {
    Ok(RecoveryExecutionRequest {
        request_id: row.get("request_id"),
        policy_id: row.get("policy_id"),
        requested_by: row.get("requested_by"),
        tenant_id: row.get("tenant_id"),
        requesting_node_id: row.get("requesting_node_id"),
        requesting_public_key_fingerprint: row.get("requesting_public_key_fingerprint"),
        artifact_request_id: row.get("artifact_request_id"),
        manifest_digest: row.get("manifest_digest"),
        safety_snapshot_receipt_id: row.get("safety_snapshot_receipt_id"),
        safety_snapshot_digest: row.get("safety_snapshot_digest"),
        rehearsal_id: row.get("rehearsal_id"),
        confirmation_expires_at: row.get("confirmation_expires_at"),
        idempotency_key: row.get("idempotency_key"),
        requested_at: row.get("requested_at"),
    })
}

pub(super) fn confirmation_from_row(
    row: &Row,
) -> Result<RecoveryConfirmation, AdministrationStoreError> {
    Ok(RecoveryConfirmation {
        confirmation_id: row.get("confirmation_id"),
        request_id: row.get("request_id"),
        outcome: RecoveryConfirmationOutcome::from_i16(row.get("outcome"))?,
        reason: row.get("reason"),
        occurred_at: row.get("occurred_at"),
        recorded_at: row.get("recorded_at"),
        confirming_signing_key_id: row.get("confirming_signing_key_id"),
        confirming_node_id: row.get("confirming_node_id"),
        confirming_public_key_fingerprint: row.get("confirming_public_key_fingerprint"),
    })
}

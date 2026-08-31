//! Bounded value objects, deterministic ids, and canonical digests for
//! ADR-0119's adopted-policy and Snapshot request/receipt records, and
//! ADR-0128's recognition of the hardened loopback profile as their verified
//! principal.

use std::time::SystemTime;

use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio_postgres::Row;

use super::export_model::{MAX_EXPORT_RECORDS, MAX_PURPOSE_BYTES, MAX_SCHEMA_VERSION_BYTES};

const MAX_IDENTIFIER_BYTES: usize = 256;
const MAX_CLASSIFICATION_BYTES: usize = 256;
const MAX_RETENTION_BYTES: usize = 4_096;
const MAX_REASON_BYTES: usize = 4_096;
const MAX_ARTIFACT_PATH_BYTES: usize = 4_096;
const DIGEST_BYTES: usize = 32;

/// ADR-0119 decision 1's closed operation vocabulary. Only `Snapshot` has a
/// store-backed execution path today; the others exist so a policy can be
/// adopted for them ahead of their own implementation without a schema
/// change, and so `AdministrationStoreError::UnknownOperation` has a real
/// closed set to validate against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdministrationOperation {
    Snapshot,
    Export,
    RecoveryExecution,
    LifecyclePurge,
}

impl AdministrationOperation {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::Snapshot => 1,
            Self::Export => 2,
            Self::RecoveryExecution => 3,
            Self::LifecyclePurge => 4,
        }
    }

    fn from_i16(value: i16) -> Result<Self, AdministrationStoreError> {
        match value {
            1 => Ok(Self::Snapshot),
            2 => Ok(Self::Export),
            3 => Ok(Self::RecoveryExecution),
            4 => Ok(Self::LifecyclePurge),
            other => Err(AdministrationStoreError::UnknownOperation { value: other }),
        }
    }
}

/// ADR-0119 decision 4: "A platform-wide snapshot is visibly platform-scoped
/// and cannot be requested under a tenant-only grant; a tenant-scoped
/// snapshot cannot include another tenant's data." The two scopes are a
/// closed pair, never a bare optional tenant id, so a request can never leave
/// its scope kind ambiguous.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdministrationScope {
    Platform,
    Tenant(String),
}

impl AdministrationScope {
    pub(super) fn kind_i16(&self) -> i16 {
        match self {
            Self::Platform => 1,
            Self::Tenant(_) => 2,
        }
    }

    pub(super) fn tenant_id(&self) -> Option<&str> {
        match self {
            Self::Platform => None,
            Self::Tenant(tenant_id) => Some(tenant_id.as_str()),
        }
    }

    /// The owned tenant id, for a caller outside this crate building its own
    /// response (a borrowed `&str` cannot outlive the `AdministrationPolicy`
    /// it is read from once that value is moved into a response struct).
    pub fn tenant_id_owned(&self) -> Option<String> {
        match self {
            Self::Platform => None,
            Self::Tenant(tenant_id) => Some(tenant_id.clone()),
        }
    }

    fn from_parts(kind: i16, tenant_id: Option<String>) -> Result<Self, AdministrationStoreError> {
        match (kind, tenant_id) {
            (1, None) => Ok(Self::Platform),
            (2, Some(tenant_id)) => Ok(Self::Tenant(tenant_id)),
            _ => Err(AdministrationStoreError::InconsistentScope),
        }
    }
}

/// An adopted-policy request (ADR-0119 decision 2's requirement that survives
/// ADR-0128: a verified principal alone never authorizes a privileged
/// operation without a stored record naming it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyAdoptionRequest {
    pub operation: AdministrationOperation,
    pub scope: AdministrationScope,
    pub data_classification: String,
    pub retention_basis: String,
    pub adopted_by: String,
    pub idempotency_key: String,
    pub effective_at: SystemTime,
    pub expires_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdministrationPolicy {
    pub policy_id: String,
    pub operation: AdministrationOperation,
    pub scope: AdministrationScope,
    pub data_classification: String,
    pub retention_basis: String,
    pub adopted_by: String,
    pub idempotency_key: String,
    pub effective_at: SystemTime,
    pub expires_at: SystemTime,
    pub revoked_at: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyWriteOutcome {
    pub policy: AdministrationPolicy,
    pub idempotent_replay: bool,
}

/// A caller's request for a new Snapshot artifact, authorized by an already
/// adopted, still-active policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSnapshotRequest {
    pub policy_id: String,
    pub requested_by: String,
    pub scope: AdministrationScope,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRequest {
    pub request_id: String,
    pub policy_id: String,
    pub requested_by: String,
    pub scope: AdministrationScope,
    pub idempotency_key: String,
    pub requested_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRequestOutcome {
    pub request: SnapshotRequest,
    pub idempotent_replay: bool,
}

/// ADR-0119 decision 4's receipt fields: what happened, not merely that a
/// request was accepted. Every field but `reason` is optional because a
/// refused or failed attempt has no artifact to describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotOutcome {
    Succeeded,
    Failed,
    Refused,
}

impl SnapshotOutcome {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSnapshotReceipt {
    pub request_id: String,
    pub outcome: SnapshotOutcome,
    pub reason: String,
    pub artifact_path: Option<String>,
    pub manifest_digest: Option<Vec<u8>>,
    pub encryption_key_id: Option<String>,
    pub size_bytes: Option<i64>,
    pub verified: bool,
    pub occurred_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotReceipt {
    pub receipt_id: String,
    pub request_id: String,
    pub outcome: SnapshotOutcome,
    pub reason: String,
    pub artifact_path: Option<String>,
    pub manifest_digest: Option<Vec<u8>>,
    pub encryption_key_id: Option<String>,
    pub size_bytes: Option<i64>,
    pub verified: bool,
    pub occurred_at: SystemTime,
    pub recorded_at: SystemTime,
}

#[derive(Debug, Error)]
pub enum AdministrationStoreError {
    #[error("administration store database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("{field} must be a bounded non-empty identifier")]
    InvalidIdentifier { field: &'static str },
    #[error("data classification must be between 1 and {MAX_CLASSIFICATION_BYTES} bytes")]
    InvalidDataClassification,
    #[error("retention basis must be between 1 and {MAX_RETENTION_BYTES} bytes")]
    InvalidRetentionBasis,
    #[error("policy expiry must be after its effective time")]
    InvalidPolicyLifetime,
    #[error("receipt reason must be at most {MAX_REASON_BYTES} bytes")]
    InvalidReason,
    #[error("artifact path must be between 1 and {MAX_ARTIFACT_PATH_BYTES} bytes")]
    InvalidArtifactPath,
    #[error("manifest digest must be exactly {DIGEST_BYTES} bytes")]
    InvalidManifestDigest,
    #[error("receipt time must not be in the future")]
    InvalidReceiptTime,
    #[error("timestamp must be at or after the Unix epoch")]
    InvalidTimestamp,
    #[error("a scope's kind and tenant id must agree")]
    InconsistentScope,
    #[error("unknown administration operation: {value}")]
    UnknownOperation { value: i16 },
    #[error("unknown outcome: {value}")]
    UnknownOutcome { value: i16 },
    #[error("the idempotency key was already used for a different policy request")]
    PolicyIdempotencyConflict,
    #[error("the idempotency key was already used for a different snapshot request")]
    RequestIdempotencyConflict,
    #[error("receipt id was replayed with different content")]
    ReceiptConflict,
    #[error("no active, unexpired, unrevoked policy authorizes this operation and scope")]
    NoActivePolicy,
    #[error("unknown administration policy: {policy_id}")]
    UnknownPolicy { policy_id: String },
    #[error("unknown snapshot request: {request_id}")]
    UnknownRequest { request_id: String },
    #[error("the confirmation window must be positive")]
    InvalidConfirmationWindow,
    #[error("unknown data category: {value}")]
    UnknownDataCategory { value: i16 },
    #[error("unknown purge request: {request_id}")]
    UnknownPurgeRequest { request_id: String },
    #[error("purpose must be between 1 and {MAX_PURPOSE_BYTES} bytes")]
    InvalidPurpose,
    #[error("max_records must be between 1 and {MAX_EXPORT_RECORDS}")]
    InvalidMaxRecords,
    #[error("schema_version must be between 1 and {MAX_SCHEMA_VERSION_BYTES} bytes")]
    InvalidSchemaVersion,
    #[error("unknown export request: {request_id}")]
    UnknownExportRequest { request_id: String },
    #[error("a legacy unsigned purge preview cannot be confirmed; create a new signed preview")]
    LegacyPurgeRequestUnauthenticated,
    #[error("the confirming signing key must differ from the key that created the preview")]
    SelfConfirmationRefused,
    #[error("a receipt must name both confirmation signing key and node, or neither")]
    IncompleteConfirmationPrincipal,
    #[error("unknown recovery execution request: {request_id}")]
    UnknownRecoveryExecutionRequest { request_id: String },
    #[error("the named artifact has no succeeded Snapshot receipt to restore")]
    UnknownRecoveryArtifact,
    #[error("the declared manifest digest does not match the artifact's own recorded receipt")]
    RecoveryArtifactManifestMismatch,
    #[error("the named rehearsal report does not exist, did not pass, or covers a different artifact digest")]
    NoPassingRehearsalForArtifact,
}

fn is_identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_IDENTIFIER_BYTES
}

pub(super) fn require_identifier(
    field: &'static str,
    value: &str,
) -> Result<(), AdministrationStoreError> {
    if is_identifier(value) {
        Ok(())
    } else {
        Err(AdministrationStoreError::InvalidIdentifier { field })
    }
}

pub(super) fn validate_policy_request(
    request: &PolicyAdoptionRequest,
) -> Result<(), AdministrationStoreError> {
    require_identifier("adopted_by", &request.adopted_by)?;
    require_identifier("idempotency_key", &request.idempotency_key)?;
    if let AdministrationScope::Tenant(tenant_id) = &request.scope {
        require_identifier("tenant_id", tenant_id)?;
    }
    if request.data_classification.is_empty()
        || request.data_classification.len() > MAX_CLASSIFICATION_BYTES
    {
        return Err(AdministrationStoreError::InvalidDataClassification);
    }
    if request.retention_basis.is_empty() || request.retention_basis.len() > MAX_RETENTION_BYTES {
        return Err(AdministrationStoreError::InvalidRetentionBasis);
    }
    if request.expires_at <= request.effective_at {
        return Err(AdministrationStoreError::InvalidPolicyLifetime);
    }
    Ok(())
}

pub(super) fn validate_snapshot_request(
    request: &NewSnapshotRequest,
) -> Result<(), AdministrationStoreError> {
    require_identifier("requested_by", &request.requested_by)?;
    require_identifier("idempotency_key", &request.idempotency_key)?;
    if let AdministrationScope::Tenant(tenant_id) = &request.scope {
        require_identifier("tenant_id", tenant_id)?;
    }
    Ok(())
}

pub(super) fn validate_snapshot_receipt(
    receipt: &NewSnapshotReceipt,
    now: SystemTime,
) -> Result<(), AdministrationStoreError> {
    if receipt.reason.len() > MAX_REASON_BYTES {
        return Err(AdministrationStoreError::InvalidReason);
    }
    if let Some(path) = &receipt.artifact_path {
        if path.is_empty() || path.len() > MAX_ARTIFACT_PATH_BYTES {
            return Err(AdministrationStoreError::InvalidArtifactPath);
        }
    }
    if let Some(digest) = &receipt.manifest_digest {
        if digest.len() != DIGEST_BYTES {
            return Err(AdministrationStoreError::InvalidManifestDigest);
        }
    }
    if receipt.occurred_at > now {
        return Err(AdministrationStoreError::InvalidReceiptTime);
    }
    Ok(())
}

/// A lost response must replay the same policy rather than adopt a second
/// one, so Ackplane derives its opaque id from the scoped idempotency
/// identity, exactly like `work_command_store::assigned_command_id`.
pub(super) fn assigned_policy_id(request: &PolicyAdoptionRequest) -> String {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.policy.id.v1",
    );
    append_bytes(&mut hasher, request.adopted_by.as_bytes());
    append_bytes(&mut hasher, request.idempotency_key.as_bytes());
    hex_id("administration-policy", hasher)
}

pub(super) fn policy_digest(
    request: &PolicyAdoptionRequest,
) -> Result<Vec<u8>, AdministrationStoreError> {
    let mut hasher = Sha256::new();
    append_bytes(&mut hasher, b"mindleak.ackplane.administration.policy.v1");
    hasher.update(request.operation.as_i16().to_be_bytes());
    hasher.update(request.scope.kind_i16().to_be_bytes());
    append_optional_bytes(&mut hasher, request.scope.tenant_id());
    append_bytes(&mut hasher, request.data_classification.as_bytes());
    append_bytes(&mut hasher, request.retention_basis.as_bytes());
    append_bytes(&mut hasher, request.adopted_by.as_bytes());
    append_bytes(&mut hasher, request.idempotency_key.as_bytes());
    append_timestamp(&mut hasher, request.effective_at)?;
    append_timestamp(&mut hasher, request.expires_at)?;
    Ok(hasher.finalize().to_vec())
}

pub(super) fn assigned_request_id(request: &NewSnapshotRequest) -> String {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.snapshot-request.id.v1",
    );
    append_bytes(&mut hasher, request.requested_by.as_bytes());
    append_bytes(&mut hasher, request.idempotency_key.as_bytes());
    hex_id("administration-snapshot-request", hasher)
}

pub(super) fn snapshot_request_digest(
    request: &NewSnapshotRequest,
) -> Result<Vec<u8>, AdministrationStoreError> {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.snapshot-request.v1",
    );
    append_bytes(&mut hasher, request.policy_id.as_bytes());
    append_bytes(&mut hasher, request.requested_by.as_bytes());
    hasher.update(request.scope.kind_i16().to_be_bytes());
    append_optional_bytes(&mut hasher, request.scope.tenant_id());
    append_bytes(&mut hasher, request.idempotency_key.as_bytes());
    Ok(hasher.finalize().to_vec())
}

pub(super) fn assigned_receipt_id(receipt: &NewSnapshotReceipt) -> String {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.snapshot-receipt.id.v1",
    );
    append_bytes(&mut hasher, receipt.request_id.as_bytes());
    hex_id("administration-snapshot-receipt", hasher)
}

pub(super) fn snapshot_receipt_digest(
    receipt: &NewSnapshotReceipt,
) -> Result<Vec<u8>, AdministrationStoreError> {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.snapshot-receipt.v1",
    );
    append_bytes(&mut hasher, receipt.request_id.as_bytes());
    hasher.update(receipt.outcome.as_i16().to_be_bytes());
    append_bytes(&mut hasher, receipt.reason.as_bytes());
    append_optional_bytes(&mut hasher, receipt.artifact_path.as_deref());
    match &receipt.manifest_digest {
        Some(digest) => {
            hasher.update([1]);
            hasher.update(digest);
        }
        None => hasher.update([0]),
    }
    append_optional_bytes(&mut hasher, receipt.encryption_key_id.as_deref());
    match receipt.size_bytes {
        Some(size) => {
            hasher.update([1]);
            hasher.update(size.to_be_bytes());
        }
        None => hasher.update([0]),
    }
    hasher.update([u8::from(receipt.verified)]);
    append_timestamp(&mut hasher, receipt.occurred_at)?;
    Ok(hasher.finalize().to_vec())
}

pub(super) fn policy_from_row(row: &Row) -> Result<AdministrationPolicy, AdministrationStoreError> {
    Ok(AdministrationPolicy {
        policy_id: row.get("policy_id"),
        operation: AdministrationOperation::from_i16(row.get("operation"))?,
        scope: AdministrationScope::from_parts(row.get("scope_kind"), row.get("tenant_id"))?,
        data_classification: row.get("data_classification"),
        retention_basis: row.get("retention_basis"),
        adopted_by: row.get("adopted_by"),
        idempotency_key: row.get("idempotency_key"),
        effective_at: row.get("effective_at"),
        expires_at: row.get("expires_at"),
        revoked_at: row.get("revoked_at"),
    })
}

pub(super) fn snapshot_request_from_row(
    row: &Row,
) -> Result<SnapshotRequest, AdministrationStoreError> {
    Ok(SnapshotRequest {
        request_id: row.get("request_id"),
        policy_id: row.get("policy_id"),
        requested_by: row.get("requested_by"),
        scope: AdministrationScope::from_parts(row.get("scope_kind"), row.get("tenant_id"))?,
        idempotency_key: row.get("idempotency_key"),
        requested_at: row.get("requested_at"),
    })
}

pub(super) fn snapshot_receipt_from_row(
    row: &Row,
) -> Result<SnapshotReceipt, AdministrationStoreError> {
    Ok(SnapshotReceipt {
        receipt_id: row.get("receipt_id"),
        request_id: row.get("request_id"),
        outcome: SnapshotOutcome::from_i16(row.get("outcome"))?,
        reason: row.get("reason"),
        artifact_path: row.get("artifact_path"),
        manifest_digest: row.get("manifest_digest"),
        encryption_key_id: row.get("encryption_key_id"),
        size_bytes: row.get("size_bytes"),
        verified: row.get("verified"),
        occurred_at: row.get("occurred_at"),
        recorded_at: row.get("recorded_at"),
    })
}

pub(super) fn hex_id(prefix: &str, hasher: Sha256) -> String {
    let hex = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("{prefix}:{hex}")
}

pub(super) fn append_bytes(hasher: &mut Sha256, value: &[u8]) {
    hasher.update((value.len() as u64).to_be_bytes());
    hasher.update(value);
}

fn append_optional_bytes(hasher: &mut Sha256, value: Option<&str>) {
    match value {
        Some(value) => {
            hasher.update([1]);
            append_bytes(hasher, value.as_bytes());
        }
        None => hasher.update([0]),
    }
}

pub(super) fn append_timestamp(
    hasher: &mut Sha256,
    timestamp: SystemTime,
) -> Result<(), AdministrationStoreError> {
    let micros: i64 = timestamp
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| AdministrationStoreError::InvalidTimestamp)?
        .as_micros()
        .try_into()
        .map_err(|_| AdministrationStoreError::InvalidTimestamp)?;
    hasher.update(micros.to_be_bytes());
    Ok(())
}

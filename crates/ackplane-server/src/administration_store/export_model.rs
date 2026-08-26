//! Bounded value objects and canonical digests for ADR-0119 decision 5's
//! Export request/receipt workflow. Deliberately one closed data category
//! today (`TelemetryEvents`), the same category Lifecycle purge uses:
//! bounded diagnostic history, not part of core coordination correctness,
//! and already redaction-friendly (`node_id`/`agent_session_id` are internal
//! identifiers a portability or audit export does not need to disclose).

use std::time::SystemTime;

use sha2::{Digest, Sha256};
use tokio_postgres::Row;

use super::model::{append_bytes, append_timestamp, hex_id, require_identifier};
use super::AdministrationStoreError;

pub(super) const MAX_PURPOSE_BYTES: usize = 4_096;
const MAX_REASON_BYTES: usize = 4_096;
const MAX_ARTIFACT_PATH_BYTES: usize = 4_096;
pub(super) const MAX_SCHEMA_VERSION_BYTES: usize = 256;
const DIGEST_BYTES: usize = 32;
/// ADR-0119 decision 5's "maximum byte/record limits" -- a caller may request
/// fewer, never more.
pub const MAX_EXPORT_RECORDS: u32 = 100_000;

/// ADR-0119 decision 1's closed Export data-category vocabulary. Mirrors
/// `PurgeDataCategory`'s shape exactly (a distinct enum, not a shared one,
/// because Export's and Purge's supported categories are not guaranteed to
/// stay identical as each grows its own future categories).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExportDataCategory {
    TelemetryEvents,
}

impl ExportDataCategory {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewExportRequest {
    pub policy_id: String,
    pub requested_by: String,
    pub tenant_id: String,
    pub repository_id: String,
    pub data_category: ExportDataCategory,
    pub purpose: String,
    pub max_records: u32,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequest {
    pub request_id: String,
    pub policy_id: String,
    pub requested_by: String,
    pub tenant_id: String,
    pub repository_id: String,
    pub data_category: ExportDataCategory,
    pub purpose: String,
    pub max_records: u32,
    pub idempotency_key: String,
    pub requested_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRequestOutcome {
    pub request: ExportRequest,
    pub idempotent_replay: bool,
}

/// ADR-0119 decision 5's receipt fields: what happened, what was omitted or
/// redacted, and where the artifact lives -- never a claim of completion
/// without them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportOutcome {
    Succeeded,
    Failed,
    Refused,
}

impl ExportOutcome {
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
pub struct NewExportReceipt {
    pub request_id: String,
    pub outcome: ExportOutcome,
    pub reason: String,
    pub artifact_path: Option<String>,
    pub manifest_digest: Option<Vec<u8>>,
    pub schema_version: String,
    pub record_count: Option<i64>,
    pub redacted_fields: Vec<String>,
    pub occurred_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReceipt {
    pub receipt_id: String,
    pub request_id: String,
    pub outcome: ExportOutcome,
    pub reason: String,
    pub artifact_path: Option<String>,
    pub manifest_digest: Option<Vec<u8>>,
    pub schema_version: String,
    pub record_count: Option<i64>,
    pub redacted_fields: Vec<String>,
    pub occurred_at: SystemTime,
    pub recorded_at: SystemTime,
}

pub(super) fn validate_request(request: &NewExportRequest) -> Result<(), AdministrationStoreError> {
    require_identifier("requested_by", &request.requested_by)?;
    require_identifier("tenant_id", &request.tenant_id)?;
    require_identifier("repository_id", &request.repository_id)?;
    require_identifier("idempotency_key", &request.idempotency_key)?;
    if request.purpose.is_empty() || request.purpose.len() > MAX_PURPOSE_BYTES {
        return Err(AdministrationStoreError::InvalidPurpose);
    }
    if request.max_records == 0 || request.max_records > MAX_EXPORT_RECORDS {
        return Err(AdministrationStoreError::InvalidMaxRecords);
    }
    Ok(())
}

pub(super) fn validate_receipt(
    receipt: &NewExportReceipt,
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
    if receipt.schema_version.is_empty() || receipt.schema_version.len() > MAX_SCHEMA_VERSION_BYTES
    {
        return Err(AdministrationStoreError::InvalidSchemaVersion);
    }
    if receipt.occurred_at > now {
        return Err(AdministrationStoreError::InvalidReceiptTime);
    }
    Ok(())
}

pub(super) fn assigned_request_id(request: &NewExportRequest) -> String {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.export-request.id.v1",
    );
    append_bytes(&mut hasher, request.requested_by.as_bytes());
    append_bytes(&mut hasher, request.idempotency_key.as_bytes());
    hex_id("administration-export-request", hasher)
}

pub(super) fn request_digest(
    request: &NewExportRequest,
) -> Result<Vec<u8>, AdministrationStoreError> {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.export-request.v1",
    );
    append_bytes(&mut hasher, request.policy_id.as_bytes());
    append_bytes(&mut hasher, request.requested_by.as_bytes());
    append_bytes(&mut hasher, request.tenant_id.as_bytes());
    append_bytes(&mut hasher, request.repository_id.as_bytes());
    hasher.update(request.data_category.as_i16().to_be_bytes());
    append_bytes(&mut hasher, request.purpose.as_bytes());
    hasher.update(request.max_records.to_be_bytes());
    append_bytes(&mut hasher, request.idempotency_key.as_bytes());
    Ok(hasher.finalize().to_vec())
}

pub(super) fn assigned_receipt_id(receipt: &NewExportReceipt) -> String {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.export-receipt.id.v1",
    );
    append_bytes(&mut hasher, receipt.request_id.as_bytes());
    hex_id("administration-export-receipt", hasher)
}

pub(super) fn receipt_digest(
    receipt: &NewExportReceipt,
) -> Result<Vec<u8>, AdministrationStoreError> {
    let mut hasher = Sha256::new();
    append_bytes(
        &mut hasher,
        b"mindleak.ackplane.administration.export-receipt.v1",
    );
    append_bytes(&mut hasher, receipt.request_id.as_bytes());
    hasher.update(receipt.outcome.as_i16().to_be_bytes());
    append_bytes(&mut hasher, receipt.reason.as_bytes());
    match &receipt.artifact_path {
        Some(path) => {
            hasher.update([1]);
            append_bytes(&mut hasher, path.as_bytes());
        }
        None => hasher.update([0]),
    }
    match &receipt.manifest_digest {
        Some(digest) => {
            hasher.update([1]);
            hasher.update(digest);
        }
        None => hasher.update([0]),
    }
    append_bytes(&mut hasher, receipt.schema_version.as_bytes());
    match receipt.record_count {
        Some(count) => {
            hasher.update([1]);
            hasher.update(count.to_be_bytes());
        }
        None => hasher.update([0]),
    }
    hasher.update((receipt.redacted_fields.len() as u64).to_be_bytes());
    for field in &receipt.redacted_fields {
        append_bytes(&mut hasher, field.as_bytes());
    }
    append_timestamp(&mut hasher, receipt.occurred_at)?;
    Ok(hasher.finalize().to_vec())
}

pub(super) fn request_from_row(row: &Row) -> Result<ExportRequest, AdministrationStoreError> {
    Ok(ExportRequest {
        request_id: row.get("request_id"),
        policy_id: row.get("policy_id"),
        requested_by: row.get("requested_by"),
        tenant_id: row.get("tenant_id"),
        repository_id: row.get("repository_id"),
        data_category: ExportDataCategory::from_i16(row.get("data_category"))?,
        purpose: row.get("purpose"),
        max_records: u32::try_from(row.get::<_, i32>("max_records")).unwrap_or(0),
        idempotency_key: row.get("idempotency_key"),
        requested_at: row.get("requested_at"),
    })
}

pub(super) fn receipt_from_row(row: &Row) -> Result<ExportReceipt, AdministrationStoreError> {
    Ok(ExportReceipt {
        receipt_id: row.get("receipt_id"),
        request_id: row.get("request_id"),
        outcome: ExportOutcome::from_i16(row.get("outcome"))?,
        reason: row.get("reason"),
        artifact_path: row.get("artifact_path"),
        manifest_digest: row.get("manifest_digest"),
        schema_version: row.get("schema_version"),
        record_count: row.get("record_count"),
        redacted_fields: row.get("redacted_fields"),
        occurred_at: row.get("occurred_at"),
        recorded_at: row.get("recorded_at"),
    })
}

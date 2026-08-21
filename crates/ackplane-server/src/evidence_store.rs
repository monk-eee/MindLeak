//! PostgreSQL-backed bounded evidence records for the Industrial Evidence Board.
//!
//! This store never receives raw terminal output, source text, credentials, or
//! a local database page. It records typed references and a SHA-256 digest so
//! later conformance and review projections can prove what they inspected.

use std::time::SystemTime;

use tokio_postgres::{Client, NoTls};

mod conformance;
mod pagination;

pub(crate) use conformance::normalize_postgres_timestamp;
pub use conformance::{
    ConformanceCursor, ConformancePage, ConformanceRecord, ConformanceReviewState,
    ConformanceStoreError, ConformanceVerdict, RecordConformanceOutcome, RecordConformanceRequest,
};
pub use pagination::{EvidenceCursor, EvidencePage};

const MIGRATION: &str = include_str!("../migrations/0014_evidence.sql");
const CONFORMANCE_MIGRATION: &str = include_str!("../migrations/0015_evidence_conformance.sql");
const SHA256_DIGEST_BYTES: usize = 32;
const MAX_TASK_ID_BYTES: usize = 256;
const MAX_SOURCE_REF_BYTES: usize = 512;
const MAX_IDENTITY_BYTES: usize = 256;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceKind {
    Commit,
    Execution,
    Receipt,
    Conformance,
    Review,
}

impl EvidenceKind {
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            1 => Some(Self::Commit),
            2 => Some(Self::Execution),
            3 => Some(Self::Receipt),
            4 => Some(Self::Conformance),
            5 => Some(Self::Review),
            _ => None,
        }
    }

    fn as_i16(self) -> i16 {
        match self {
            Self::Commit => 1,
            Self::Execution => 2,
            Self::Receipt => 3,
            Self::Conformance => 4,
            Self::Review => 5,
        }
    }

    fn from_i16(value: i16) -> Option<Self> {
        Self::from_i32(i32::from(value))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EvidenceStoreError {
    #[error("evidence database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("task_id must be between 1 and {MAX_TASK_ID_BYTES} bytes")]
    InvalidTaskId,
    #[error("source_ref must be between 1 and {MAX_SOURCE_REF_BYTES} bytes")]
    InvalidSourceRef,
    #[error("content_digest must be exactly {SHA256_DIGEST_BYTES} bytes")]
    InvalidDigest,
    #[error("reported_agent_session_id and recorded_by must be between 1 and {MAX_IDENTITY_BYTES} bytes")]
    InvalidIdentity,
    #[error("page cursor must carry a bounded evidence identifier")]
    InvalidCursor,
    #[error(
        "idempotency_key must be between 1 and {MAX_IDEMPOTENCY_KEY_BYTES} bytes when supplied"
    )]
    InvalidIdempotencyKey,
    #[error("idempotency_key was already used for a different evidence record")]
    IdempotencyConflict,
    #[error("stored evidence kind {0} is outside the EvidenceKind contract")]
    UnknownStoredKind(i16),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordEvidenceRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub task_id: String,
    pub kind: EvidenceKind,
    pub source_ref: String,
    pub content_digest: Vec<u8>,
    pub observed_at: SystemTime,
    pub reported_agent_session_id: String,
    pub recorded_by: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRecord {
    pub evidence_id: String,
    pub tenant_id: String,
    pub repository_id: String,
    pub task_id: String,
    pub kind: EvidenceKind,
    pub source_ref: String,
    pub content_digest: Vec<u8>,
    pub observed_at: SystemTime,
    pub reported_agent_session_id: String,
    pub recorded_by: String,
    pub recorded_at: SystemTime,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordEvidenceOutcome {
    pub record: EvidenceRecord,
    pub idempotent_replay: bool,
}

pub struct EvidenceStore {
    client: Client,
}

impl EvidenceStore {
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (mut client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane evidence store connection closed with an error");
            }
        });
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::EVIDENCE,
            MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::EVIDENCE_CONFORMANCE,
            CONFORMANCE_MIGRATION,
        )
        .await?;
        Ok(Self { client })
    }

    pub async fn resolve_signing_key(
        &self,
        binding: &crate::signing_keys::EnvelopeBinding<'_>,
    ) -> Result<crate::signing_keys::KeyResolution, crate::signing_keys::SigningKeyError> {
        crate::signing_keys::resolve(&self.client, binding).await
    }

    pub async fn consume_evidence_nonce(
        &self,
        signing_key_id: &str,
        nonce: &[u8],
        now: SystemTime,
    ) -> Result<bool, EvidenceStoreError> {
        let inserted = self
            .client
            .execute(
                "INSERT INTO evidence_authentication_nonces (signing_key_id, nonce, consumed_at) \
                 VALUES ($1, $2, $3) ON CONFLICT (signing_key_id, nonce) DO NOTHING",
                &[&signing_key_id, &nonce, &now],
            )
            .await?;
        Ok(inserted == 1)
    }

    pub async fn record(
        &self,
        request: RecordEvidenceRequest,
    ) -> Result<RecordEvidenceOutcome, EvidenceStoreError> {
        let request = RecordEvidenceRequest {
            observed_at: normalize_postgres_timestamp(request.observed_at),
            ..request
        };
        if !request.idempotency_key.is_empty() {
            if let Some(existing) = self
                .find_evidence_by_idempotency(
                    &request.tenant_id,
                    &request.repository_id,
                    &request.idempotency_key,
                )
                .await?
            {
                return evidence_outcome_for_existing(existing, &request);
            }
        }
        validate_request(&request)?;
        let evidence_id = unique_evidence_id();
        let row = self
            .client
            .query_opt(
                "INSERT INTO evidence_records (
                     tenant_id, repository_id, evidence_id, task_id, evidence_kind,
                     source_ref, content_digest, observed_at, reported_agent_session_id, recorded_by,
                     idempotency_key
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
                 ON CONFLICT (tenant_id, repository_id, idempotency_key)
                     WHERE idempotency_key IS NOT NULL DO NOTHING
                 RETURNING recorded_at",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &evidence_id,
                    &request.task_id,
                    &request.kind.as_i16(),
                    &request.source_ref,
                    &request.content_digest,
                    &request.observed_at,
                    &request.reported_agent_session_id,
                    &request.recorded_by,
                    &(!request.idempotency_key.is_empty())
                        .then_some(request.idempotency_key.as_str()),
                ],
            )
            .await?;
        let Some(row) = row else {
            let existing = self
                .find_evidence_by_idempotency(
                    &request.tenant_id,
                    &request.repository_id,
                    &request.idempotency_key,
                )
                .await?
                .ok_or(EvidenceStoreError::IdempotencyConflict)?;
            return evidence_outcome_for_existing(existing, &request);
        };
        Ok(RecordEvidenceOutcome {
            record: EvidenceRecord {
                evidence_id,
                tenant_id: request.tenant_id,
                repository_id: request.repository_id,
                task_id: request.task_id,
                kind: request.kind,
                source_ref: request.source_ref,
                content_digest: request.content_digest,
                observed_at: request.observed_at,
                reported_agent_session_id: request.reported_agent_session_id,
                recorded_by: request.recorded_by,
                recorded_at: row.get("recorded_at"),
                idempotency_key: request.idempotency_key,
            },
            idempotent_replay: false,
        })
    }

    pub async fn find_evidence_by_idempotency(
        &self,
        tenant_id: &str,
        repository_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<EvidenceRecord>, EvidenceStoreError> {
        if !is_bounded(idempotency_key, MAX_IDEMPOTENCY_KEY_BYTES) {
            return Err(EvidenceStoreError::InvalidIdempotencyKey);
        }
        self.client
            .query_opt(
                "SELECT evidence_id, tenant_id, repository_id, task_id, evidence_kind, \
                        source_ref, content_digest, observed_at, reported_agent_session_id, recorded_by, \
                        recorded_at, idempotency_key \
                 FROM evidence_records \
                 WHERE tenant_id = $1 AND repository_id = $2 AND idempotency_key = $3",
                &[&tenant_id, &repository_id, &idempotency_key],
            )
            .await?
            .map(|row| row_to_evidence(&row))
            .transpose()
    }

    pub async fn list(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
        limit: i64,
    ) -> Result<Vec<EvidenceRecord>, EvidenceStoreError> {
        if !is_bounded(task_id, MAX_TASK_ID_BYTES) || limit < 1 {
            return Err(EvidenceStoreError::InvalidTaskId);
        }
        let rows = self
            .client
            .query(
                "SELECT evidence_id, tenant_id, repository_id, task_id, evidence_kind, \
                    source_ref, content_digest, observed_at, reported_agent_session_id, recorded_by, \
                    recorded_at, idempotency_key \
                 FROM evidence_records \
                 WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 \
                 ORDER BY recorded_at DESC, evidence_id ASC \
                 LIMIT $4",
                &[&tenant_id, &repository_id, &task_id, &limit],
            )
            .await?;
        rows.iter().map(row_to_evidence).collect()
    }
}

pub(crate) fn evidence_outcome_for_existing(
    existing: EvidenceRecord,
    request: &RecordEvidenceRequest,
) -> Result<RecordEvidenceOutcome, EvidenceStoreError> {
    let matches = existing.task_id == request.task_id
        && existing.kind == request.kind
        && existing.source_ref == request.source_ref
        && existing.content_digest == request.content_digest
        && existing.observed_at == request.observed_at
        && existing.reported_agent_session_id == request.reported_agent_session_id
        && existing.recorded_by == request.recorded_by;
    if !matches {
        return Err(EvidenceStoreError::IdempotencyConflict);
    }
    Ok(RecordEvidenceOutcome {
        record: existing,
        idempotent_replay: true,
    })
}

pub(super) fn row_to_evidence(
    row: &tokio_postgres::Row,
) -> Result<EvidenceRecord, EvidenceStoreError> {
    let stored_kind: i16 = row.get("evidence_kind");
    let kind = EvidenceKind::from_i16(stored_kind)
        .ok_or(EvidenceStoreError::UnknownStoredKind(stored_kind))?;
    Ok(EvidenceRecord {
        evidence_id: row.get("evidence_id"),
        tenant_id: row.get("tenant_id"),
        repository_id: row.get("repository_id"),
        task_id: row.get("task_id"),
        kind,
        source_ref: row.get("source_ref"),
        content_digest: row.get("content_digest"),
        observed_at: row.get("observed_at"),
        reported_agent_session_id: row.get("reported_agent_session_id"),
        recorded_by: row.get("recorded_by"),
        recorded_at: row.get("recorded_at"),
        idempotency_key: row
            .get::<_, Option<String>>("idempotency_key")
            .unwrap_or_default(),
    })
}

fn validate_request(request: &RecordEvidenceRequest) -> Result<(), EvidenceStoreError> {
    if !is_bounded(&request.task_id, MAX_TASK_ID_BYTES) {
        return Err(EvidenceStoreError::InvalidTaskId);
    }
    if !valid_source_ref(request.kind, &request.source_ref) {
        return Err(EvidenceStoreError::InvalidSourceRef);
    }
    if request.content_digest.len() != SHA256_DIGEST_BYTES {
        return Err(EvidenceStoreError::InvalidDigest);
    }
    if !is_bounded(&request.reported_agent_session_id, MAX_IDENTITY_BYTES)
        || !is_bounded(&request.recorded_by, MAX_IDENTITY_BYTES)
    {
        return Err(EvidenceStoreError::InvalidIdentity);
    }
    if !request.idempotency_key.is_empty()
        && !is_bounded(&request.idempotency_key, MAX_IDEMPOTENCY_KEY_BYTES)
    {
        return Err(EvidenceStoreError::InvalidIdempotencyKey);
    }
    Ok(())
}

fn is_bounded(value: &str, max_bytes: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max_bytes
}

fn valid_source_ref(kind: EvidenceKind, source_ref: &str) -> bool {
    let prefix = match kind {
        EvidenceKind::Commit => "commit:",
        EvidenceKind::Execution => "execution:",
        EvidenceKind::Receipt => "receipt:",
        EvidenceKind::Conformance => "conformance:",
        EvidenceKind::Review => "review:",
    };
    let Some(identifier) = source_ref.strip_prefix(prefix) else {
        return false;
    };
    is_bounded(source_ref, MAX_SOURCE_REF_BYTES)
        && !identifier.is_empty()
        && identifier.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'/')
        })
}

fn unique_evidence_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("the OS random source should be available");
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("evidence-{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_scope(label: &str) -> (String, String) {
        let mut bytes = [0u8; 8];
        getrandom::getrandom(&mut bytes).expect("the OS random source should be available");
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        (
            format!("tenant-evidence-{label}-{hex}"),
            format!("repository-evidence-{label}-{hex}"),
        )
    }

    async fn store() -> Option<EvidenceStore> {
        let database_url = std::env::var("ACKPLANE_TEST_DATABASE_URL").ok()?;
        Some(EvidenceStore::connect(&database_url).await.unwrap())
    }

    fn request(tenant_id: String, repository_id: String, task_id: &str) -> RecordEvidenceRequest {
        RecordEvidenceRequest {
            tenant_id,
            repository_id,
            task_id: task_id.to_string(),
            kind: EvidenceKind::Commit,
            source_ref: "commit:0123456789abcdef".to_string(),
            content_digest: vec![7; SHA256_DIGEST_BYTES],
            observed_at: SystemTime::UNIX_EPOCH,
            reported_agent_session_id: "session:v1:evidence-test".to_string(),
            recorded_by: "node:evidence-test".to_string(),
            idempotency_key: String::new(),
        }
    }

    #[tokio::test]
    async fn records_and_lists_task_evidence_without_crossing_tenant_scope() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("list");
        let recorded = store
            .record(request(
                tenant_id.clone(),
                repository_id.clone(),
                "task:123",
            ))
            .await
            .unwrap();
        let (other_tenant_id, other_repository_id) = unique_scope("other");
        store
            .record(request(other_tenant_id, other_repository_id, "task:123"))
            .await
            .unwrap();

        let records = store
            .list(&tenant_id, &repository_id, "task:123", 10)
            .await
            .unwrap();

        assert_eq!(records, vec![recorded.record]);
    }

    #[tokio::test]
    async fn keyset_pagination_reaches_later_evidence_without_repeating_the_first_page() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("pagination");
        for _ in 0..3 {
            store
                .record(request(
                    tenant_id.clone(),
                    repository_id.clone(),
                    "task:123",
                ))
                .await
                .unwrap();
        }

        let first = store
            .list_page(&tenant_id, &repository_id, "task:123", None, 1)
            .await
            .unwrap();
        let cursor = first
            .next_cursor
            .clone()
            .expect("a full first page must return a cursor");
        let second = store
            .list_page(&tenant_id, &repository_id, "task:123", Some(&cursor), 1)
            .await
            .unwrap();

        assert_eq!(first.entries.len(), 1);
        assert_eq!(second.entries.len(), 1);
        assert_ne!(first.entries[0].evidence_id, second.entries[0].evidence_id);
    }

    #[test]
    fn rejects_an_evidence_record_without_a_sha256_digest() {
        let (tenant_id, repository_id) = unique_scope("digest");
        let mut invalid = request(tenant_id, repository_id, "task:123");
        invalid.content_digest = vec![0; SHA256_DIGEST_BYTES - 1];

        assert!(matches!(
            validate_request(&invalid),
            Err(EvidenceStoreError::InvalidDigest)
        ));
    }

    #[test]
    fn rejects_a_source_reference_that_is_not_typed_for_its_evidence_kind() {
        let (tenant_id, repository_id) = unique_scope("source-ref");
        let mut invalid = request(tenant_id, repository_id, "task:123");
        invalid.source_ref = "terminal output must not be evidence storage".to_string();

        assert!(matches!(
            validate_request(&invalid),
            Err(EvidenceStoreError::InvalidSourceRef)
        ));
    }
}

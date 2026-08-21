//! Tenant-scoped read access for the Industrial Evidence Board.
//!
//! The Bridge deliberately reuses Ackplane's typed Evidence store rather
//! than opening a raw SQL route or copying evidence bodies into browser state.

use std::error::Error;
use std::time::SystemTime;

use ackplane_server::evidence_store::{
    ConformanceCursor, ConformancePage, ConformanceRecord, ConformanceReviewState,
    ConformanceStoreError, ConformanceVerdict, EvidenceCursor, EvidenceKind, EvidencePage,
    EvidenceRecord, EvidenceStore, EvidenceStoreError,
};

pub const DEFAULT_PAGE_SIZE: u32 = 20;
pub const MAX_PAGE_SIZE: u32 = 100;

/// Browser-safe projection of a typed Evidence record.
///
/// The record keeps its bounded source reference and digest, but deliberately
/// has no field for terminal output, source text, credentials, or a database
/// export payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceView {
    pub evidence_id: String,
    pub task_id: String,
    pub kind: &'static str,
    pub source_ref: String,
    pub content_digest_hex: String,
    pub observed_at: SystemTime,
    pub reported_agent_session_id: String,
    pub recorded_by: String,
    pub recorded_at: SystemTime,
    pub receipt_id: Option<String>,
}

impl From<EvidenceRecord> for EvidenceView {
    fn from(record: EvidenceRecord) -> Self {
        let receipt_id = matches!(record.kind, EvidenceKind::Receipt)
            .then(|| {
                record
                    .source_ref
                    .strip_prefix("receipt:")
                    .map(str::to_owned)
            })
            .flatten();
        Self {
            evidence_id: record.evidence_id,
            task_id: record.task_id,
            kind: evidence_kind_label(record.kind),
            source_ref: record.source_ref,
            content_digest_hex: digest_hex(&record.content_digest),
            observed_at: record.observed_at,
            reported_agent_session_id: record.reported_agent_session_id,
            recorded_by: record.recorded_by,
            recorded_at: record.recorded_at,
            receipt_id,
        }
    }
}

/// Browser-safe projection of an Evidence-linked conformance outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceView {
    pub conformance_id: String,
    pub task_id: String,
    pub evidence_id: String,
    pub verdict: &'static str,
    pub finding_count: u32,
    pub findings_digest_hex: String,
    pub review_state: &'static str,
    pub reported_checked_at: SystemTime,
    pub evaluated_by: String,
    pub recorded_at: SystemTime,
}

impl From<ConformanceRecord> for ConformanceView {
    fn from(record: ConformanceRecord) -> Self {
        Self {
            conformance_id: record.conformance_id,
            task_id: record.task_id,
            evidence_id: record.evidence_id,
            verdict: verdict_label(record.verdict),
            finding_count: record.finding_count,
            findings_digest_hex: digest_hex(&record.findings_digest),
            review_state: review_state_label(record.review_state),
            reported_checked_at: record.reported_checked_at,
            evaluated_by: record.evaluated_by,
            recorded_at: record.recorded_at,
        }
    }
}

/// Read-only Evidence Board data source for one Bridge process.
pub struct BridgeEvidenceStore {
    store: EvidenceStore,
}

impl BridgeEvidenceStore {
    pub async fn connect(
        database_url: &str,
    ) -> Result<Self, Box<dyn Error + Send + Sync + 'static>> {
        Ok(Self {
            store: EvidenceStore::connect(database_url).await?,
        })
    }

    /// Returns one stable keyset page of typed evidence for a tenant-owned task.
    pub async fn task_evidence(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
        cursor: Option<&EvidenceCursor>,
        requested_limit: Option<u32>,
    ) -> Result<EvidencePage, EvidenceStoreError> {
        self.store
            .list_page(
                tenant_id,
                repository_id,
                task_id,
                cursor,
                page_limit(requested_limit),
            )
            .await
    }

    /// Returns one stable keyset page of derived conformance history for the
    /// same tenant-owned task. Findings remain a count and digest, never body text.
    pub async fn task_conformance(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
        cursor: Option<&ConformanceCursor>,
        requested_limit: Option<u32>,
    ) -> Result<ConformancePage, ConformanceStoreError> {
        self.store
            .list_conformance_page(
                tenant_id,
                repository_id,
                task_id,
                cursor,
                page_limit(requested_limit),
            )
            .await
    }
}

pub fn page_limit(requested_limit: Option<u32>) -> i64 {
    i64::from(
        requested_limit
            .unwrap_or(DEFAULT_PAGE_SIZE)
            .clamp(1, MAX_PAGE_SIZE),
    )
}

fn digest_hex(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn evidence_kind_label(kind: EvidenceKind) -> &'static str {
    match kind {
        EvidenceKind::Commit => "commit",
        EvidenceKind::Execution => "execution",
        EvidenceKind::Receipt => "receipt",
        EvidenceKind::Conformance => "conformance",
        EvidenceKind::Review => "review",
    }
}

fn verdict_label(verdict: ConformanceVerdict) -> &'static str {
    match verdict {
        ConformanceVerdict::Aligned => "aligned",
        ConformanceVerdict::Drift => "drift",
        ConformanceVerdict::Violation => "violation",
        ConformanceVerdict::NeedsHuman => "needs_human",
    }
}

fn review_state_label(state: ConformanceReviewState) -> &'static str {
    match state {
        ConformanceReviewState::NotRequired => "not_required",
        ConformanceReviewState::Pending => "pending",
        ConformanceReviewState::Blocked => "blocked",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_limit_uses_the_default_and_bounds_requested_values() {
        for (requested, expected) in [
            (None, i64::from(DEFAULT_PAGE_SIZE)),
            (Some(0), 1),
            (Some(42), 42),
            (Some(MAX_PAGE_SIZE + 1), i64::from(MAX_PAGE_SIZE)),
        ] {
            assert_eq!(page_limit(requested), expected);
        }
    }

    #[test]
    fn receipt_evidence_projection_keeps_typed_navigation_without_a_body() {
        let timestamp = SystemTime::UNIX_EPOCH;
        let view = EvidenceView::from(EvidenceRecord {
            evidence_id: "evidence-1".to_string(),
            tenant_id: "tenant-1".to_string(),
            repository_id: "repository-1".to_string(),
            task_id: "task:1".to_string(),
            kind: EvidenceKind::Receipt,
            source_ref: "receipt:ack-123".to_string(),
            content_digest: vec![0x0a, 0xff],
            observed_at: timestamp,
            reported_agent_session_id: "session:v1:reported".to_string(),
            recorded_by: "node:1".to_string(),
            recorded_at: timestamp,
            idempotency_key: "evidence:1".to_string(),
        });

        assert_eq!(
            view,
            EvidenceView {
                evidence_id: "evidence-1".to_string(),
                task_id: "task:1".to_string(),
                kind: "receipt",
                source_ref: "receipt:ack-123".to_string(),
                content_digest_hex: "0aff".to_string(),
                observed_at: timestamp,
                reported_agent_session_id: "session:v1:reported".to_string(),
                recorded_by: "node:1".to_string(),
                recorded_at: timestamp,
                receipt_id: Some("ack-123".to_string()),
            }
        );
    }

    #[test]
    fn conformance_projection_exposes_the_reviewable_outcome_not_findings_body() {
        let timestamp = SystemTime::UNIX_EPOCH;
        let view = ConformanceView::from(ConformanceRecord {
            conformance_id: "conformance-1".to_string(),
            tenant_id: "tenant-1".to_string(),
            repository_id: "repository-1".to_string(),
            task_id: "task:1".to_string(),
            evidence_id: "evidence-1".to_string(),
            verdict: ConformanceVerdict::NeedsHuman,
            finding_count: 3,
            findings_digest: vec![0x12, 0x34],
            review_state: ConformanceReviewState::Pending,
            reported_checked_at: timestamp,
            evaluated_by: "node:1".to_string(),
            recorded_at: timestamp,
            idempotency_key: "conformance:1".to_string(),
        });

        assert_eq!(
            view,
            ConformanceView {
                conformance_id: "conformance-1".to_string(),
                task_id: "task:1".to_string(),
                evidence_id: "evidence-1".to_string(),
                verdict: "needs_human",
                finding_count: 3,
                findings_digest_hex: "1234".to_string(),
                review_state: "pending",
                reported_checked_at: timestamp,
                evaluated_by: "node:1".to_string(),
                recorded_at: timestamp,
            }
        );
    }
}

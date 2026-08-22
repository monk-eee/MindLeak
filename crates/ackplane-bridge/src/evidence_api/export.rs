//! Redacted, tenant-scoped Evidence Board audit export.
//!
//! Export remains a read: Bridge produces a bounded audit package from
//! Ackplane's typed records and never writes a workstation file, reveals an
//! evidence body, or exposes raw session labels.

use std::time::SystemTime;

use ackplane_server::evidence_store::{ConformanceCursor, EvidenceCursor};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::evidence::{page_limit, ConformanceView, EvidenceView};

use super::{
    conformance_store_error, encode_cursor, ensure_repository_visible, evidence_store_error,
    parse_cursor, unix_seconds, EvidenceApiState,
};

const EXPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Deserialize)]
pub(super) struct EvidenceExportQuery {
    limit: Option<u32>,
    agent_id: Option<String>,
    evidence_cursor: Option<String>,
    conformance_cursor: Option<String>,
}

#[derive(Serialize)]
pub(super) struct EvidenceExportResponse {
    schema_version: u32,
    exported_at_seconds: Option<u64>,
    tenant_id: String,
    repository_id: String,
    task_id: String,
    agent_id: Option<String>,
    effective_limit: i64,
    redaction: &'static str,
    evidence_complete: bool,
    conformance_complete: bool,
    next_evidence_cursor: Option<String>,
    next_conformance_cursor: Option<String>,
    evidence: Vec<AuditEvidenceEntry>,
    conformance: Vec<AuditConformanceEntry>,
}

#[derive(Serialize)]
struct AuditEvidenceEntry {
    evidence_id: String,
    task_id: String,
    kind: &'static str,
    source_ref: String,
    content_digest_hex: String,
    observed_at_seconds: Option<u64>,
    recorded_by: String,
    reported_agent_session_fingerprint: String,
    reported_constitution_version: Option<String>,
    recorded_at_seconds: Option<u64>,
    receipt_id: Option<String>,
}

impl From<EvidenceView> for AuditEvidenceEntry {
    fn from(view: EvidenceView) -> Self {
        Self {
            evidence_id: view.evidence_id,
            task_id: view.task_id,
            kind: view.kind,
            source_ref: view.source_ref,
            content_digest_hex: view.content_digest_hex,
            observed_at_seconds: unix_seconds(view.observed_at),
            recorded_by: view.recorded_by,
            reported_agent_session_fingerprint: redact_session_label(
                &view.reported_agent_session_id,
            ),
            reported_constitution_version: view.reported_constitution_version,
            recorded_at_seconds: unix_seconds(view.recorded_at),
            receipt_id: view.receipt_id,
        }
    }
}

#[derive(Serialize)]
struct AuditConformanceEntry {
    conformance_id: String,
    task_id: String,
    evidence_id: String,
    verdict: &'static str,
    finding_count: u32,
    findings_digest_hex: String,
    finding_codes: Vec<String>,
    review_state: &'static str,
    reported_checked_at_seconds: Option<u64>,
    evaluated_by: String,
    reported_constitution_version: Option<String>,
    recorded_at_seconds: Option<u64>,
}

impl From<ConformanceView> for AuditConformanceEntry {
    fn from(view: ConformanceView) -> Self {
        Self {
            conformance_id: view.conformance_id,
            task_id: view.task_id,
            evidence_id: view.evidence_id,
            verdict: view.verdict,
            finding_count: view.finding_count,
            findings_digest_hex: view.findings_digest_hex,
            finding_codes: view.finding_codes,
            review_state: view.review_state,
            reported_checked_at_seconds: unix_seconds(view.reported_checked_at),
            evaluated_by: view.evaluated_by,
            reported_constitution_version: view.reported_constitution_version,
            recorded_at_seconds: unix_seconds(view.recorded_at),
        }
    }
}

pub(super) async fn evidence_export(
    State(state): State<EvidenceApiState>,
    Path((repository_id, task_id)): Path<(String, String)>,
    Query(query): Query<EvidenceExportQuery>,
) -> Result<Json<EvidenceExportResponse>, StatusCode> {
    // The same tenant visibility guard as every Evidence route is the export
    // authorization boundary in the loopback developer profile.
    ensure_repository_visible(&state, &repository_id).await?;
    let evidence_cursor =
        parse_cursor(query.evidence_cursor.as_deref())?.map(|(recorded_at, evidence_id)| {
            EvidenceCursor {
                recorded_at,
                evidence_id,
            }
        });
    let conformance_cursor =
        parse_cursor(query.conformance_cursor.as_deref())?.map(|(recorded_at, conformance_id)| {
            ConformanceCursor {
                recorded_at,
                conformance_id,
            }
        });
    let effective_limit = page_limit(query.limit);
    let evidence_page = state
        .evidence
        .task_evidence(
            state.tenant_id.as_ref(),
            &repository_id,
            &task_id,
            query.agent_id.as_deref(),
            evidence_cursor.as_ref(),
            query.limit,
        )
        .await
        .map_err(evidence_store_error)?;
    let conformance_page = state
        .evidence
        .task_conformance(
            state.tenant_id.as_ref(),
            &repository_id,
            &task_id,
            query.agent_id.as_deref(),
            conformance_cursor.as_ref(),
            query.limit,
        )
        .await
        .map_err(conformance_store_error)?;
    let next_evidence_cursor = evidence_page
        .next_cursor
        .map(|cursor| encode_cursor(cursor.recorded_at, cursor.evidence_id))
        .transpose()?;
    let next_conformance_cursor = conformance_page
        .next_cursor
        .map(|cursor| encode_cursor(cursor.recorded_at, cursor.conformance_id))
        .transpose()?;

    Ok(Json(EvidenceExportResponse {
        schema_version: EXPORT_SCHEMA_VERSION,
        exported_at_seconds: unix_seconds(SystemTime::now()),
        tenant_id: state.tenant_id.to_string(),
        repository_id,
        task_id,
        agent_id: query.agent_id,
        effective_limit,
        redaction: "evidence bodies, finding bodies, credentials, raw session labels, and idempotency keys are omitted",
        evidence_complete: next_evidence_cursor.is_none(),
        conformance_complete: next_conformance_cursor.is_none(),
        next_evidence_cursor,
        next_conformance_cursor,
        evidence: evidence_page
            .entries
            .into_iter()
            .map(EvidenceView::from)
            .map(AuditEvidenceEntry::from)
            .collect(),
        conformance: conformance_page
            .entries
            .into_iter()
            .map(ConformanceView::from)
            .map(AuditConformanceEntry::from)
            .collect(),
    }))
}

fn redact_session_label(session_label: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(session_label.as_bytes());
    let digest = digest.finalize();
    format!("sha256:{}", hex_lower(&digest))
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_redaction_is_deterministic_and_never_returns_the_raw_label() {
        let raw = "session:v1:operator-private-label";
        let first = redact_session_label(raw);
        let second = redact_session_label(raw);

        assert_eq!(first, second);
        assert_ne!(first, raw);
        assert_eq!(
            first,
            "sha256:ebebf7a1bd0e27f3af4c7337494592fbea1b4c626f2128c9a6e9ffd45183c994"
        );
    }
}

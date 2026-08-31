//! Tenant-scoped detail reads for one conformance outcome.

use super::super::{is_bounded, EvidenceStore, MAX_TASK_ID_BYTES};
use super::{row_to_conformance, ConformanceRecord, ConformanceStoreError, MAX_EVIDENCE_ID_BYTES};

impl EvidenceStore {
    pub async fn conformance_detail(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
        conformance_id: &str,
    ) -> Result<Option<ConformanceRecord>, ConformanceStoreError> {
        if !is_bounded(task_id, MAX_TASK_ID_BYTES) {
            return Err(ConformanceStoreError::InvalidTaskId);
        }
        if !is_bounded(conformance_id, MAX_EVIDENCE_ID_BYTES) {
            return Err(ConformanceStoreError::InvalidEvidenceId);
        }
        self.pool
            .get()
            .await?
            .query_opt(
                "SELECT conformance_id, tenant_id, repository_id, task_id, evidence_id, verdict, \
                        finding_count, findings_digest, finding_codes, review_state, reported_checked_at, \
                        evaluated_by, recorded_at, idempotency_key, reported_constitution_version \
                 FROM conformance_records \
                 WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 AND conformance_id = $4",
                &[&tenant_id, &repository_id, &task_id, &conformance_id],
            )
            .await?
            .map(|row| row_to_conformance(&row))
            .transpose()
    }
}

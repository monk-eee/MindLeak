//! Tenant-scoped detail reads for one Evidence record.

use super::{
    is_bounded, row_to_evidence, EvidenceRecord, EvidenceStore, EvidenceStoreError,
    MAX_TASK_ID_BYTES,
};

impl EvidenceStore {
    pub async fn evidence_detail(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
        evidence_id: &str,
    ) -> Result<Option<EvidenceRecord>, EvidenceStoreError> {
        if !is_bounded(task_id, MAX_TASK_ID_BYTES) {
            return Err(EvidenceStoreError::InvalidTaskId);
        }
        if !is_bounded(evidence_id, MAX_TASK_ID_BYTES) {
            return Err(EvidenceStoreError::InvalidEvidenceId);
        }
        self.connection()
            .await?
            .query_opt(
                "SELECT evidence_id, tenant_id, repository_id, task_id, evidence_kind, \
                        source_ref, content_digest, observed_at, reported_agent_session_id, recorded_by, \
                        recorded_at, idempotency_key, reported_constitution_version \
                 FROM evidence_records \
                 WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 AND evidence_id = $4",
                &[&tenant_id, &repository_id, &task_id, &evidence_id],
            )
            .await?
            .map(|row| row_to_evidence(&row))
            .transpose()
    }
}

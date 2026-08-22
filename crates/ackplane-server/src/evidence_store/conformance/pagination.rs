//! Stable task-history reads for conformance outcomes.

use super::super::{is_bounded, EvidenceStore, MAX_TASK_ID_BYTES};
use super::{
    row_to_conformance, ConformanceCursor, ConformancePage, ConformanceRecord,
    ConformanceStoreError, MAX_EVIDENCE_ID_BYTES,
};

impl EvidenceStore {
    pub async fn list_conformance(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
        limit: i64,
    ) -> Result<Vec<ConformanceRecord>, ConformanceStoreError> {
        if !is_bounded(task_id, MAX_TASK_ID_BYTES) || limit < 1 {
            return Err(ConformanceStoreError::InvalidTaskId);
        }
        let rows = self
            .client
            .query(
                "SELECT conformance_id, tenant_id, repository_id, task_id, evidence_id, verdict, \
                      finding_count, findings_digest, finding_codes, review_state, reported_checked_at, evaluated_by, \
                       recorded_at, idempotency_key, reported_constitution_version \
                 FROM conformance_records \
                 WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 \
                   ORDER BY recorded_at DESC, conformance_id ASC \
                 LIMIT $4",
                &[&tenant_id, &repository_id, &task_id, &limit],
            )
            .await?;
        rows.iter().map(row_to_conformance).collect()
    }

    pub async fn list_conformance_page(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
        cursor: Option<&ConformanceCursor>,
        limit: i64,
    ) -> Result<ConformancePage, ConformanceStoreError> {
        if !is_bounded(task_id, MAX_TASK_ID_BYTES) || limit < 1 {
            return Err(ConformanceStoreError::InvalidTaskId);
        }
        if cursor.is_some_and(|cursor| !is_bounded(&cursor.conformance_id, MAX_EVIDENCE_ID_BYTES)) {
            return Err(ConformanceStoreError::InvalidEvidenceId);
        }
        let fetch_limit = limit
            .checked_add(1)
            .ok_or(ConformanceStoreError::InvalidEvidenceId)?;
        let rows = match cursor {
            Some(cursor) => {
                self.client
                    .query(
                        "SELECT conformance_id, tenant_id, repository_id, task_id, evidence_id, verdict, \
                            finding_count, findings_digest, finding_codes, review_state, reported_checked_at, evaluated_by, \
                                recorded_at, idempotency_key, reported_constitution_version \
                         FROM conformance_records \
                         WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 \
                           AND (recorded_at < $4 OR (recorded_at = $4 AND conformance_id > $5)) \
                         ORDER BY recorded_at DESC, conformance_id ASC \
                         LIMIT $6",
                        &[
                            &tenant_id,
                            &repository_id,
                            &task_id,
                            &cursor.recorded_at,
                            &cursor.conformance_id,
                            &fetch_limit,
                        ],
                    )
                    .await?
            }
            None => {
                self.client
                    .query(
                        "SELECT conformance_id, tenant_id, repository_id, task_id, evidence_id, verdict, \
                            finding_count, findings_digest, finding_codes, review_state, reported_checked_at, evaluated_by, \
                                recorded_at, idempotency_key, reported_constitution_version \
                         FROM conformance_records \
                         WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 \
                         ORDER BY recorded_at DESC, conformance_id ASC \
                         LIMIT $4",
                        &[&tenant_id, &repository_id, &task_id, &fetch_limit],
                    )
                    .await?
            }
        };
        let mut entries = rows
            .iter()
            .map(row_to_conformance)
            .collect::<Result<Vec<_>, ConformanceStoreError>>()?;
        let limit = usize::try_from(limit).map_err(|_| ConformanceStoreError::InvalidEvidenceId)?;
        let next_cursor = if entries.len() > limit {
            entries.truncate(limit);
            entries.last().map(|entry| ConformanceCursor {
                recorded_at: entry.recorded_at,
                conformance_id: entry.conformance_id.clone(),
            })
        } else {
            None
        };
        Ok(ConformancePage {
            entries,
            next_cursor,
        })
    }
}

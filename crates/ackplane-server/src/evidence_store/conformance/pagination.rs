//! Stable task-history reads for conformance outcomes.

use super::super::{is_bounded, EvidenceStore, MAX_IDENTITY_BYTES, MAX_TASK_ID_BYTES};
use super::{
    row_to_conformance, ConformanceCursor, ConformanceHistoryFilter, ConformancePage,
    ConformanceRecord, ConformanceReviewState, ConformanceStoreError, MAX_EVIDENCE_ID_BYTES,
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
            .pool
            .get()
            .await?
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
        filter: ConformanceHistoryFilter<'_>,
        cursor: Option<&ConformanceCursor>,
        limit: i64,
    ) -> Result<ConformancePage, ConformanceStoreError> {
        if !is_bounded(task_id, MAX_TASK_ID_BYTES) || limit < 1 {
            return Err(ConformanceStoreError::InvalidTaskId);
        }
        if filter
            .agent_id
            .is_some_and(|agent_id| !is_bounded(agent_id, MAX_IDENTITY_BYTES))
        {
            return Err(ConformanceStoreError::InvalidEvaluator);
        }
        if cursor.is_some_and(|cursor| !is_bounded(&cursor.conformance_id, MAX_EVIDENCE_ID_BYTES)) {
            return Err(ConformanceStoreError::InvalidEvidenceId);
        }
        let fetch_limit = limit
            .checked_add(1)
            .ok_or(ConformanceStoreError::InvalidEvidenceId)?;
        let agent_id = filter.agent_id;
        let review_state = filter.review_state.map(ConformanceReviewState::as_i16);
        let connection = self.pool.get().await?;
        let rows = match cursor {
            Some(cursor) => {
                connection
                    .query(
                        "SELECT conformance_id, tenant_id, repository_id, task_id, evidence_id, verdict, \
                            finding_count, findings_digest, finding_codes, review_state, reported_checked_at, evaluated_by, \
                                recorded_at, idempotency_key, reported_constitution_version \
                         FROM conformance_records \
                         WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 \
                                    AND ($4::TEXT IS NULL OR evaluated_by = $4) \
                                    AND ($5::SMALLINT IS NULL OR review_state = $5) \
                                    AND (recorded_at < $6 OR (recorded_at = $6 AND conformance_id > $7)) \
                         ORDER BY recorded_at DESC, conformance_id ASC \
                                 LIMIT $8",
                        &[
                            &tenant_id,
                            &repository_id,
                            &task_id,
                            &agent_id,
                                     &review_state,
                            &cursor.recorded_at,
                            &cursor.conformance_id,
                            &fetch_limit,
                        ],
                    )
                    .await?
            }
            None => {
                connection
                    .query(
                        "SELECT conformance_id, tenant_id, repository_id, task_id, evidence_id, verdict, \
                            finding_count, findings_digest, finding_codes, review_state, reported_checked_at, evaluated_by, \
                                recorded_at, idempotency_key, reported_constitution_version \
                         FROM conformance_records \
                         WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 \
                                                     AND ($4::TEXT IS NULL OR evaluated_by = $4) \
                                                     AND ($5::SMALLINT IS NULL OR review_state = $5) \
                         ORDER BY recorded_at DESC, conformance_id ASC \
                                                 LIMIT $6",
                                                &[
                                                        &tenant_id,
                                                        &repository_id,
                                                        &task_id,
                                                        &agent_id,
                                                        &review_state,
                                                        &fetch_limit,
                                                ],
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

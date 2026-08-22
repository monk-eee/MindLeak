//! Stable keyset pagination for task-bound Evidence records.

use std::time::SystemTime;

use super::{
    is_bounded, row_to_evidence, EvidenceRecord, EvidenceStore, EvidenceStoreError,
    MAX_IDENTITY_BYTES, MAX_TASK_ID_BYTES,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceCursor {
    pub recorded_at: SystemTime,
    pub evidence_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidencePage {
    pub entries: Vec<EvidenceRecord>,
    pub next_cursor: Option<EvidenceCursor>,
}

impl EvidenceStore {
    pub async fn list_page(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
        agent_id: Option<&str>,
        cursor: Option<&EvidenceCursor>,
        limit: i64,
    ) -> Result<EvidencePage, EvidenceStoreError> {
        if !is_bounded(task_id, MAX_TASK_ID_BYTES) || limit < 1 {
            return Err(EvidenceStoreError::InvalidTaskId);
        }
        if agent_id.is_some_and(|agent_id| !is_bounded(agent_id, MAX_IDENTITY_BYTES)) {
            return Err(EvidenceStoreError::InvalidIdentity);
        }
        if cursor.is_some_and(|cursor| !is_bounded(&cursor.evidence_id, MAX_TASK_ID_BYTES)) {
            return Err(EvidenceStoreError::InvalidCursor);
        }
        let fetch_limit = limit
            .checked_add(1)
            .ok_or(EvidenceStoreError::InvalidCursor)?;
        let rows = match cursor {
            Some(cursor) => {
                self.client
                    .query(
                        "SELECT evidence_id, tenant_id, repository_id, task_id, evidence_kind, \
                                source_ref, content_digest, observed_at, reported_agent_session_id, recorded_by, \
                                recorded_at, idempotency_key, reported_constitution_version \
                         FROM evidence_records \
                         WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 \
                                                     AND ($4::TEXT IS NULL OR recorded_by = $4) \
                                                     AND (recorded_at < $5 OR (recorded_at = $5 AND evidence_id > $6)) \
                         ORDER BY recorded_at DESC, evidence_id ASC \
                                                 LIMIT $7",
                        &[
                            &tenant_id,
                            &repository_id,
                            &task_id,
                            &agent_id,
                            &cursor.recorded_at,
                            &cursor.evidence_id,
                            &fetch_limit,
                        ],
                    )
                    .await?
            }
            None => {
                self.client
                    .query(
                        "SELECT evidence_id, tenant_id, repository_id, task_id, evidence_kind, \
                                source_ref, content_digest, observed_at, reported_agent_session_id, recorded_by, \
                                recorded_at, idempotency_key, reported_constitution_version \
                         FROM evidence_records \
                         WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3 \
                                                     AND ($4::TEXT IS NULL OR recorded_by = $4) \
                         ORDER BY recorded_at DESC, evidence_id ASC \
                                                 LIMIT $5",
                                                &[&tenant_id, &repository_id, &task_id, &agent_id, &fetch_limit],
                    )
                    .await?
            }
        };
        let mut entries = rows
            .iter()
            .map(row_to_evidence)
            .collect::<Result<Vec<_>, EvidenceStoreError>>()?;
        let limit = usize::try_from(limit).map_err(|_| EvidenceStoreError::InvalidCursor)?;
        let next_cursor = if entries.len() > limit {
            entries.truncate(limit);
            entries.last().map(|entry| EvidenceCursor {
                recorded_at: entry.recorded_at,
                evidence_id: entry.evidence_id.clone(),
            })
        } else {
            None
        };
        Ok(EvidencePage {
            entries,
            next_cursor,
        })
    }
}

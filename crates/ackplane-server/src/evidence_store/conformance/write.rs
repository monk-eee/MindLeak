//! Transactional conformance record persistence (`record_conformance`,
//! `find_conformance_by_idempotency`), split out of `conformance.rs` to keep
//! it under this repository's module-length ratchet, matching the existing
//! model/validation-vs-persistence seam other stores (e.g. `DelegationStore`)
//! already draw.

use super::{
    is_bounded, normalize_postgres_timestamp, outcome_for_existing, row_to_conformance,
    unique_conformance_id, validate_request, ConformanceRecord, ConformanceStoreError,
    RecordConformanceOutcome, RecordConformanceRequest, MAX_IDEMPOTENCY_KEY_BYTES,
};
use crate::evidence_store::EvidenceStore;

impl EvidenceStore {
    pub async fn record_conformance(
        &self,
        request: RecordConformanceRequest,
    ) -> Result<RecordConformanceOutcome, ConformanceStoreError> {
        let request = RecordConformanceRequest {
            reported_checked_at: normalize_postgres_timestamp(request.reported_checked_at),
            ..request
        };
        if let Some(existing) = self
            .find_conformance_by_idempotency(
                &request.tenant_id,
                &request.repository_id,
                &request.idempotency_key,
            )
            .await?
        {
            return outcome_for_existing(existing, &request);
        }
        validate_request(&request)?;
        let connection = self.connection().await?;
        let evidence_task = connection
            .query_opt(
                "SELECT task_id, recorded_by FROM evidence_records \
                 WHERE tenant_id = $1 AND repository_id = $2 AND evidence_id = $3",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &request.evidence_id,
                ],
            )
            .await?
            .ok_or(ConformanceStoreError::MissingEvidence)?;
        let evidence_task_id: String = evidence_task.get("task_id");
        if evidence_task_id != request.task_id {
            return Err(ConformanceStoreError::EvidenceTaskMismatch);
        }
        let evidence_producer: String = evidence_task.get("recorded_by");
        if evidence_producer != request.evaluated_by {
            return Err(ConformanceStoreError::EvidenceProducerMismatch);
        }

        let conformance_id = unique_conformance_id();
        let review_state = request.verdict.review_state();
        let finding_codes = request
            .finding_codes
            .iter()
            .map(|code| code.as_i16())
            .collect::<Vec<_>>();
        let row = connection
            .query_opt(
                "INSERT INTO conformance_records (
                     tenant_id, repository_id, conformance_id, task_id, evidence_id,
                     verdict, finding_count, findings_digest, finding_codes, review_state, reported_checked_at,
                     evaluated_by, idempotency_key, reported_constitution_version
                 ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)
                 ON CONFLICT (tenant_id, repository_id, idempotency_key)
                     WHERE idempotency_key IS NOT NULL DO NOTHING
                 RETURNING recorded_at",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &conformance_id,
                    &request.task_id,
                    &request.evidence_id,
                    &request.verdict.as_i16(),
                    &i64::from(request.finding_count),
                    &request.findings_digest,
                    &finding_codes,
                    &review_state.as_i16(),
                    &request.reported_checked_at,
                    &request.evaluated_by,
                    &request.idempotency_key,
                    &request.reported_constitution_version,
                ],
            )
            .await?;
        let Some(row) = row else {
            let existing = self
                .find_conformance_by_idempotency(
                    &request.tenant_id,
                    &request.repository_id,
                    &request.idempotency_key,
                )
                .await?
                .ok_or(ConformanceStoreError::IdempotencyConflict)?;
            return outcome_for_existing(existing, &request);
        };
        Ok(RecordConformanceOutcome {
            record: ConformanceRecord {
                conformance_id,
                tenant_id: request.tenant_id,
                repository_id: request.repository_id,
                task_id: request.task_id,
                evidence_id: request.evidence_id,
                verdict: request.verdict,
                finding_count: request.finding_count,
                findings_digest: request.findings_digest,
                finding_codes: request.finding_codes,
                review_state,
                reported_checked_at: request.reported_checked_at,
                evaluated_by: request.evaluated_by,
                recorded_at: row.get("recorded_at"),
                idempotency_key: request.idempotency_key,
                reported_constitution_version: request.reported_constitution_version,
            },
            idempotent_replay: false,
        })
    }

    pub async fn find_conformance_by_idempotency(
        &self,
        tenant_id: &str,
        repository_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<ConformanceRecord>, ConformanceStoreError> {
        if !is_bounded(idempotency_key, MAX_IDEMPOTENCY_KEY_BYTES) {
            return Err(ConformanceStoreError::InvalidIdempotencyKey);
        }
        self.connection()
            .await?
            .query_opt(
                "SELECT conformance_id, tenant_id, repository_id, task_id, evidence_id, verdict, \
                    finding_count, findings_digest, finding_codes, review_state, reported_checked_at, evaluated_by, \
                        recorded_at, idempotency_key, reported_constitution_version \
                 FROM conformance_records \
                 WHERE tenant_id = $1 AND repository_id = $2 AND idempotency_key = $3",
                &[&tenant_id, &repository_id, &idempotency_key],
            )
            .await?
            .map(|row| row_to_conformance(&row))
            .transpose()
    }
}

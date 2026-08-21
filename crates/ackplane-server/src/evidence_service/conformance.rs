//! Signed conformance outcome transport for the Evidence Board.

use ackplane_protocol::evidence_auth::EvidenceOperation;
use ackplane_protocol::v1;
use tonic::{Request, Response, Status};

use crate::evidence_signature::EvidenceAuthRefusal;
use crate::evidence_store::{
    normalize_postgres_timestamp, ConformanceCursor, ConformanceRecord, ConformanceReviewState,
    ConformanceStoreError, ConformanceVerdict, RecordConformanceRequest,
};

use super::{effective_limit, parse_cursor, parse_rfc3339, rfc3339, EvidenceGrpcService};

pub(super) fn conformance_store_error(error: ConformanceStoreError) -> Status {
    match error {
        ConformanceStoreError::InvalidTaskId => {
            Status::invalid_argument("task_id must be a bounded non-empty identifier")
        }
        ConformanceStoreError::InvalidEvidenceId => {
            Status::invalid_argument("evidence_id must be a bounded non-empty identifier")
        }
        ConformanceStoreError::InvalidFindingsDigest => {
            Status::invalid_argument("findings_digest must be exactly 32 SHA-256 bytes")
        }
        ConformanceStoreError::InvalidEvaluator => {
            Status::invalid_argument("evaluated_by must be a bounded identity")
        }
        ConformanceStoreError::InvalidConstitutionVersion => {
            Status::invalid_argument("reported_constitution_version must be bounded when supplied")
        }
        ConformanceStoreError::InvalidIdempotencyKey => {
            Status::invalid_argument("idempotency_key must be a bounded non-empty identifier")
        }
        ConformanceStoreError::MissingEvidence => {
            Status::not_found("the referenced evidence record was not found")
        }
        ConformanceStoreError::EvidenceTaskMismatch => {
            Status::invalid_argument("the referenced evidence belongs to a different task")
        }
        ConformanceStoreError::EvidenceProducerMismatch => {
            Status::permission_denied("only the evidence-producing node may report its conformance")
        }
        ConformanceStoreError::IdempotencyConflict => Status::already_exists(
            "idempotency_key was already used for a different conformance result",
        ),
        ConformanceStoreError::UnknownStoredVerdict(verdict) => {
            Status::internal(format!("stored conformance verdict {verdict} is invalid"))
        }
        ConformanceStoreError::UnknownStoredReviewState(state) => Status::internal(format!(
            "stored conformance review state {state} is invalid"
        )),
        ConformanceStoreError::InconsistentReviewState => {
            Status::internal("stored conformance review state contradicts its verdict")
        }
        ConformanceStoreError::InvalidStoredFindingCount(count) => Status::internal(format!(
            "stored conformance finding count {count} is invalid"
        )),
        ConformanceStoreError::Database(error) => Status::internal(error.to_string()),
    }
}

fn conformance_verdict(raw: i32) -> Result<ConformanceVerdict, Status> {
    ConformanceVerdict::from_i32(raw)
        .ok_or_else(|| Status::invalid_argument("conformance verdict is invalid"))
}

fn proto_verdict(verdict: ConformanceVerdict) -> i32 {
    match verdict {
        ConformanceVerdict::Aligned => v1::ConformanceVerdict::Aligned as i32,
        ConformanceVerdict::Drift => v1::ConformanceVerdict::Drift as i32,
        ConformanceVerdict::Violation => v1::ConformanceVerdict::Violation as i32,
        ConformanceVerdict::NeedsHuman => v1::ConformanceVerdict::NeedsHuman as i32,
    }
}

fn proto_review_state(state: ConformanceReviewState) -> i32 {
    match state {
        ConformanceReviewState::NotRequired => v1::ConformanceReviewState::NotRequired as i32,
        ConformanceReviewState::Pending => v1::ConformanceReviewState::Pending as i32,
        ConformanceReviewState::Blocked => v1::ConformanceReviewState::Blocked as i32,
    }
}

fn to_proto_conformance(
    record: ConformanceRecord,
    idempotent_replay: bool,
) -> Result<v1::ConformanceRecord, String> {
    Ok(v1::ConformanceRecord {
        conformance_id: record.conformance_id,
        tenant_id: record.tenant_id,
        repository_id: record.repository_id,
        task_id: record.task_id,
        evidence_id: record.evidence_id,
        verdict: proto_verdict(record.verdict),
        finding_count: record.finding_count,
        findings_digest: record.findings_digest,
        review_state: proto_review_state(record.review_state),
        reported_checked_at: rfc3339(record.reported_checked_at)?,
        evaluated_by: record.evaluated_by,
        recorded_at: rfc3339(record.recorded_at)?,
        idempotency_key: record.idempotency_key,
        idempotent_replay,
        reported_constitution_version: record.reported_constitution_version.unwrap_or_default(),
    })
}

impl EvidenceGrpcService {
    pub(super) async fn record_conformance_impl(
        &self,
        request: Request<v1::RecordConformanceRequest>,
    ) -> Result<Response<v1::ConformanceRecord>, Status> {
        let request = request.into_inner();
        let verdict = conformance_verdict(request.verdict)?;
        let operation = EvidenceOperation::RecordConformance {
            task_id: &request.task_id,
            evidence_id: &request.evidence_id,
            verdict: request.verdict,
            finding_count: request.finding_count,
            findings_digest: &request.findings_digest,
            reported_checked_at: &request.reported_checked_at,
            idempotency_key: &request.idempotency_key,
            reported_constitution_version: &request.reported_constitution_version,
        };
        let Some(authentication) = request.authentication.as_ref() else {
            return Err(Status::unauthenticated(
                EvidenceAuthRefusal::Unsigned.diagnostic(),
            ));
        };
        let evaluated_by = self
            .verify_authentication(
                &request.tenant_id,
                &request.repository_id,
                &operation,
                Some(authentication),
            )
            .await?;
        let reported_checked_at =
            normalize_postgres_timestamp(parse_rfc3339(&request.reported_checked_at)?);
        let conformance_request = RecordConformanceRequest {
            tenant_id: request.tenant_id,
            repository_id: request.repository_id,
            task_id: request.task_id,
            evidence_id: request.evidence_id,
            verdict,
            finding_count: request.finding_count,
            findings_digest: request.findings_digest,
            reported_checked_at,
            evaluated_by,
            idempotency_key: request.idempotency_key,
            reported_constitution_version: (!request.reported_constitution_version.is_empty())
                .then_some(request.reported_constitution_version),
        };
        if let Some(existing) = self
            .store
            .lock()
            .await
            .find_conformance_by_idempotency(
                &conformance_request.tenant_id,
                &conformance_request.repository_id,
                &conformance_request.idempotency_key,
            )
            .await
            .map_err(conformance_store_error)?
        {
            let matches = existing.task_id == conformance_request.task_id
                && existing.evidence_id == conformance_request.evidence_id
                && existing.verdict == conformance_request.verdict
                && existing.finding_count == conformance_request.finding_count
                && existing.findings_digest == conformance_request.findings_digest
                && existing.reported_checked_at == conformance_request.reported_checked_at
                && existing.evaluated_by == conformance_request.evaluated_by
                && existing.reported_constitution_version
                    == conformance_request.reported_constitution_version;
            if !matches {
                return Err(Status::already_exists(
                    "idempotency_key was already used for a different conformance result",
                ));
            }
            return Ok(Response::new(
                to_proto_conformance(existing, true).map_err(Status::internal)?,
            ));
        }
        self.consume_nonce(authentication).await?;
        let outcome = self
            .store
            .lock()
            .await
            .record_conformance(conformance_request)
            .await
            .map_err(conformance_store_error)?;
        Ok(Response::new(
            to_proto_conformance(outcome.record, outcome.idempotent_replay)
                .map_err(Status::internal)?,
        ))
    }

    pub(super) async fn list_conformance_impl(
        &self,
        request: Request<v1::ListConformanceRequest>,
    ) -> Result<Response<v1::ListConformanceResult>, Status> {
        let request = request.into_inner();
        let operation = EvidenceOperation::ListConformance {
            task_id: &request.task_id,
            limit: request.limit,
            page_before_recorded_at: (!request.page_before_recorded_at.is_empty())
                .then_some(request.page_before_recorded_at.as_str()),
            page_before_conformance_id: (!request.page_before_conformance_id.is_empty())
                .then_some(request.page_before_conformance_id.as_str()),
        };
        self.authenticate(
            &request.tenant_id,
            &request.repository_id,
            &operation,
            request.authentication.as_ref(),
        )
        .await?;
        let limit = effective_limit(request.limit);
        let cursor = parse_cursor(
            &request.page_before_recorded_at,
            &request.page_before_conformance_id,
            "page_before_recorded_at",
            "page_before_conformance_id",
        )?
        .map(|(recorded_at, conformance_id)| ConformanceCursor {
            recorded_at,
            conformance_id,
        });
        let page = self
            .store
            .lock()
            .await
            .list_conformance_page(
                &request.tenant_id,
                &request.repository_id,
                &request.task_id,
                cursor.as_ref(),
                i64::from(limit),
            )
            .await
            .map_err(conformance_store_error)?;
        Ok(Response::new(v1::ListConformanceResult {
            entries: page
                .entries
                .into_iter()
                .map(|record| to_proto_conformance(record, false))
                .collect::<Result<Vec<_>, String>>()
                .map_err(Status::internal)?,
            effective_limit: limit,
            next_recorded_at: page
                .next_cursor
                .as_ref()
                .map(|cursor| rfc3339(cursor.recorded_at))
                .transpose()
                .map_err(Status::internal)?
                .unwrap_or_default(),
            next_conformance_id: page
                .next_cursor
                .map(|cursor| cursor.conformance_id)
                .unwrap_or_default(),
        }))
    }
}

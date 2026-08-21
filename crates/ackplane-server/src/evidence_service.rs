//! Authenticated gRPC transport for bounded Evidence Board records.
//!
//! EvidenceService accepts only typed references and SHA-256 digests. It does
//! not accept raw terminal output, source text, credentials, or local database
//! contents as an evidence transport shortcut.

use std::sync::Arc;
use std::time::SystemTime;

use ackplane_protocol::evidence_auth::EvidenceOperation;
use ackplane_protocol::v1;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use crate::evidence_signature::{self, EvidenceAuthRefusal};
use crate::evidence_store::{
    evidence_outcome_for_existing, normalize_postgres_timestamp, EvidenceCursor, EvidenceKind,
    EvidenceRecord, EvidenceStore, EvidenceStoreError, RecordEvidenceRequest,
};

mod conformance;

const DEFAULT_EVIDENCE_LIMIT: u32 = 20;
const MAX_EVIDENCE_LIMIT: u32 = 100;

pub struct EvidenceGrpcService {
    store: Arc<Mutex<EvidenceStore>>,
}

impl EvidenceGrpcService {
    pub fn new(store: EvidenceStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }

    async fn verify_authentication(
        &self,
        tenant_id: &str,
        repository_id: &str,
        operation: &EvidenceOperation<'_>,
        authentication: Option<&v1::EvidenceAuthentication>,
    ) -> Result<String, Status> {
        let Some(authentication) = authentication else {
            return Err(Status::unauthenticated(
                EvidenceAuthRefusal::Unsigned.diagnostic(),
            ));
        };
        let binding = crate::signing_keys::EnvelopeBinding {
            signing_key_id: &authentication.signing_key_id,
            tenant_id,
            repository_id,
            producer_id: &authentication.node_id,
            accepted_at: SystemTime::now(),
        };
        let resolution = {
            let store = self.store.lock().await;
            store
                .resolve_signing_key(&binding)
                .await
                .map_err(|error| Status::internal(error.to_string()))?
        };
        evidence_signature::verify(
            tenant_id,
            repository_id,
            operation,
            Some(authentication),
            &resolution,
            SystemTime::now(),
        )
        .map_err(|refusal| {
            if refusal.is_authenticated_but_not_authorized() {
                Status::permission_denied(refusal.diagnostic())
            } else {
                Status::unauthenticated(refusal.diagnostic())
            }
        })?;

        Ok(authentication.node_id.clone())
    }

    async fn consume_nonce(
        &self,
        authentication: &v1::EvidenceAuthentication,
    ) -> Result<(), Status> {
        let fresh = {
            let store = self.store.lock().await;
            store
                .consume_evidence_nonce(
                    &authentication.signing_key_id,
                    &authentication.nonce,
                    SystemTime::now(),
                )
                .await
                .map_err(|error| Status::internal(error.to_string()))?
        };
        if !fresh {
            return Err(Status::unauthenticated(
                EvidenceAuthRefusal::Replayed.diagnostic(),
            ));
        }
        Ok(())
    }

    async fn authenticate(
        &self,
        tenant_id: &str,
        repository_id: &str,
        operation: &EvidenceOperation<'_>,
        authentication: Option<&v1::EvidenceAuthentication>,
    ) -> Result<String, Status> {
        let Some(authentication) = authentication else {
            return Err(Status::unauthenticated(
                EvidenceAuthRefusal::Unsigned.diagnostic(),
            ));
        };
        let node_id = self
            .verify_authentication(tenant_id, repository_id, operation, Some(authentication))
            .await?;
        self.consume_nonce(authentication).await?;
        Ok(node_id)
    }
}

fn store_error(error: EvidenceStoreError) -> Status {
    match error {
        EvidenceStoreError::InvalidTaskId => {
            Status::invalid_argument("task_id must be a bounded non-empty identifier")
        }
        EvidenceStoreError::InvalidSourceRef => {
            Status::invalid_argument("source_ref must be a bounded non-empty reference")
        }
        EvidenceStoreError::InvalidDigest => {
            Status::invalid_argument("content_digest must be exactly 32 SHA-256 bytes")
        }
        EvidenceStoreError::InvalidIdentity => Status::invalid_argument(
            "reported_agent_session_id and recorded_by must be bounded identities",
        ),
        EvidenceStoreError::InvalidConstitutionVersion => {
            Status::invalid_argument("reported_constitution_version must be bounded when supplied")
        }
        EvidenceStoreError::InvalidCursor => {
            Status::invalid_argument("the Evidence page cursor is invalid")
        }
        EvidenceStoreError::InvalidIdempotencyKey => {
            Status::invalid_argument("idempotency_key must be bounded when supplied")
        }
        EvidenceStoreError::IdempotencyConflict => Status::already_exists(
            "idempotency_key was already used for a different evidence record",
        ),
        EvidenceStoreError::UnknownStoredKind(kind) => {
            Status::internal(format!("stored evidence kind {kind} is invalid"))
        }
        EvidenceStoreError::Database(error) => Status::internal(error.to_string()),
    }
}

fn evidence_kind(raw: i32) -> Result<EvidenceKind, Status> {
    EvidenceKind::from_i32(raw).ok_or_else(|| Status::invalid_argument("evidence kind is invalid"))
}

fn effective_limit(requested: u32) -> u32 {
    if requested == 0 {
        DEFAULT_EVIDENCE_LIMIT
    } else {
        requested.min(MAX_EVIDENCE_LIMIT)
    }
}

fn parse_rfc3339(raw: &str) -> Result<SystemTime, Status> {
    OffsetDateTime::parse(raw, &Rfc3339)
        .map(Into::into)
        .map_err(|_| Status::invalid_argument("observed_at must be a valid RFC3339 timestamp"))
}

fn parse_cursor(
    timestamp: &str,
    identifier: &str,
    timestamp_label: &str,
    identifier_label: &str,
) -> Result<Option<(SystemTime, String)>, Status> {
    match (timestamp.is_empty(), identifier.is_empty()) {
        (true, true) => Ok(None),
        (false, false) => {
            let timestamp = OffsetDateTime::parse(timestamp, &Rfc3339)
                .map(Into::into)
                .map_err(|_| {
                    Status::invalid_argument(format!("{timestamp_label} must be RFC3339"))
                })?;
            Ok(Some((timestamp, identifier.to_owned())))
        }
        _ => Err(Status::invalid_argument(format!(
            "{timestamp_label} and {identifier_label} must be supplied together"
        ))),
    }
}

fn rfc3339(timestamp: SystemTime) -> Result<String, String> {
    OffsetDateTime::from(timestamp)
        .format(&Rfc3339)
        .map_err(|error| format!("could not format an evidence timestamp: {error}"))
}

fn proto_kind(kind: EvidenceKind) -> i32 {
    match kind {
        EvidenceKind::Commit => v1::EvidenceKind::Commit as i32,
        EvidenceKind::Execution => v1::EvidenceKind::Execution as i32,
        EvidenceKind::Receipt => v1::EvidenceKind::Receipt as i32,
        EvidenceKind::Conformance => v1::EvidenceKind::Conformance as i32,
        EvidenceKind::Review => v1::EvidenceKind::Review as i32,
    }
}

fn to_proto(record: EvidenceRecord, idempotent_replay: bool) -> Result<v1::EvidenceRecord, String> {
    Ok(v1::EvidenceRecord {
        evidence_id: record.evidence_id,
        tenant_id: record.tenant_id,
        repository_id: record.repository_id,
        task_id: record.task_id,
        kind: proto_kind(record.kind),
        source_ref: record.source_ref,
        content_digest: record.content_digest,
        observed_at: rfc3339(record.observed_at)?,
        reported_agent_session_id: record.reported_agent_session_id,
        recorded_by: record.recorded_by,
        recorded_at: rfc3339(record.recorded_at)?,
        idempotent_replay,
        reported_constitution_version: record.reported_constitution_version.unwrap_or_default(),
    })
}

#[tonic::async_trait]
impl v1::evidence_service_server::EvidenceService for EvidenceGrpcService {
    async fn record_evidence(
        &self,
        request: Request<v1::RecordEvidenceRequest>,
    ) -> Result<Response<v1::EvidenceRecord>, Status> {
        let request = request.into_inner();
        let kind = evidence_kind(request.kind)?;
        let operation = EvidenceOperation::Record {
            task_id: &request.task_id,
            evidence_kind: request.kind,
            source_ref: &request.source_ref,
            content_digest: &request.content_digest,
            observed_at: &request.observed_at,
            reported_agent_session_id: &request.reported_agent_session_id,
            idempotency_key: &request.idempotency_key,
            reported_constitution_version: &request.reported_constitution_version,
        };
        let Some(authentication) = request.authentication.as_ref() else {
            return Err(Status::unauthenticated(
                EvidenceAuthRefusal::Unsigned.diagnostic(),
            ));
        };
        let recorded_by = self
            .verify_authentication(
                &request.tenant_id,
                &request.repository_id,
                &operation,
                Some(authentication),
            )
            .await?;
        let observed_at = normalize_postgres_timestamp(parse_rfc3339(&request.observed_at)?);
        let evidence_request = RecordEvidenceRequest {
            tenant_id: request.tenant_id,
            repository_id: request.repository_id,
            task_id: request.task_id,
            kind,
            source_ref: request.source_ref,
            content_digest: request.content_digest,
            observed_at,
            reported_agent_session_id: request.reported_agent_session_id,
            recorded_by,
            idempotency_key: request.idempotency_key,
            reported_constitution_version: (!request.reported_constitution_version.is_empty())
                .then_some(request.reported_constitution_version),
        };
        if !evidence_request.idempotency_key.is_empty() {
            if let Some(existing) = self
                .store
                .lock()
                .await
                .find_evidence_by_idempotency(
                    &evidence_request.tenant_id,
                    &evidence_request.repository_id,
                    &evidence_request.idempotency_key,
                )
                .await
                .map_err(store_error)?
            {
                let outcome = evidence_outcome_for_existing(existing, &evidence_request)
                    .map_err(store_error)?;
                return Ok(Response::new(
                    to_proto(outcome.record, outcome.idempotent_replay)
                        .map_err(Status::internal)?,
                ));
            }
        }
        self.consume_nonce(authentication).await?;
        let outcome = self
            .store
            .lock()
            .await
            .record(evidence_request)
            .await
            .map_err(store_error)?;
        Ok(Response::new(
            to_proto(outcome.record, outcome.idempotent_replay).map_err(Status::internal)?,
        ))
    }

    async fn list_evidence(
        &self,
        request: Request<v1::ListEvidenceRequest>,
    ) -> Result<Response<v1::ListEvidenceResult>, Status> {
        let request = request.into_inner();
        let operation = EvidenceOperation::List {
            task_id: &request.task_id,
            limit: request.limit,
            page_before_recorded_at: (!request.page_before_recorded_at.is_empty())
                .then_some(request.page_before_recorded_at.as_str()),
            page_before_evidence_id: (!request.page_before_evidence_id.is_empty())
                .then_some(request.page_before_evidence_id.as_str()),
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
            &request.page_before_evidence_id,
            "page_before_recorded_at",
            "page_before_evidence_id",
        )?
        .map(|(recorded_at, evidence_id)| EvidenceCursor {
            recorded_at,
            evidence_id,
        });
        let page = self
            .store
            .lock()
            .await
            .list_page(
                &request.tenant_id,
                &request.repository_id,
                &request.task_id,
                cursor.as_ref(),
                i64::from(limit),
            )
            .await
            .map_err(store_error)?;
        Ok(Response::new(v1::ListEvidenceResult {
            entries: page
                .entries
                .into_iter()
                .map(|record| to_proto(record, false))
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
            next_evidence_id: page
                .next_cursor
                .map(|cursor| cursor.evidence_id)
                .unwrap_or_default(),
        }))
    }

    async fn record_conformance(
        &self,
        request: Request<v1::RecordConformanceRequest>,
    ) -> Result<Response<v1::ConformanceRecord>, Status> {
        self.record_conformance_impl(request).await
    }

    async fn list_conformance(
        &self,
        request: Request<v1::ListConformanceRequest>,
    ) -> Result<Response<v1::ListConformanceResult>, Status> {
        self.list_conformance_impl(request).await
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;
    use crate::signing_keys::{self, SigningKeyRecord};
    use ackplane_protocol::v1::evidence_service_server::EvidenceService;

    fn conformance_request(
        identity: &TestIdentity,
        task_id: &str,
        evidence_id: &str,
        verdict: v1::ConformanceVerdict,
        nonce_byte: u8,
    ) -> v1::RecordConformanceRequest {
        conformance_request_with_idempotency(
            identity,
            task_id,
            evidence_id,
            verdict,
            &format!("conformance:{nonce_byte}"),
            nonce_byte,
        )
    }

    fn conformance_request_with_idempotency(
        identity: &TestIdentity,
        task_id: &str,
        evidence_id: &str,
        verdict: v1::ConformanceVerdict,
        idempotency_key: &str,
        nonce_byte: u8,
    ) -> v1::RecordConformanceRequest {
        conformance_request_with_idempotency_and_time(
            identity,
            task_id,
            evidence_id,
            verdict,
            idempotency_key,
            "2026-01-01T00:00:00Z",
            nonce_byte,
        )
    }

    fn conformance_request_with_idempotency_and_time(
        identity: &TestIdentity,
        task_id: &str,
        evidence_id: &str,
        verdict: v1::ConformanceVerdict,
        idempotency_key: &str,
        reported_checked_at: &str,
        nonce_byte: u8,
    ) -> v1::RecordConformanceRequest {
        let key = signing_key();
        let mut authentication = v1::EvidenceAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
            nonce: vec![nonce_byte; 16],
            signature: Vec::new(),
        };
        let findings_digest = vec![8; 32];
        let reported_constitution_version = "constitution:v4";
        let operation = EvidenceOperation::RecordConformance {
            task_id,
            evidence_id,
            verdict: verdict as i32,
            finding_count: 2,
            findings_digest: &findings_digest,
            reported_checked_at,
            idempotency_key,
            reported_constitution_version,
        };
        let bytes = evidence_signature::evidence_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        v1::RecordConformanceRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            task_id: task_id.to_owned(),
            evidence_id: evidence_id.to_owned(),
            verdict: verdict as i32,
            finding_count: 2,
            findings_digest,
            reported_checked_at: reported_checked_at.to_owned(),
            authentication: Some(authentication),
            idempotency_key: idempotency_key.to_owned(),
            reported_constitution_version: reported_constitution_version.to_owned(),
        }
    }

    fn list_conformance_request(
        identity: &TestIdentity,
        task_id: &str,
        limit: u32,
        nonce_byte: u8,
    ) -> v1::ListConformanceRequest {
        let key = signing_key();
        let mut authentication = v1::EvidenceAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
            nonce: vec![nonce_byte; 16],
            signature: Vec::new(),
        };
        let operation = EvidenceOperation::ListConformance {
            task_id,
            limit,
            page_before_recorded_at: None,
            page_before_conformance_id: None,
        };
        let bytes = evidence_signature::evidence_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        v1::ListConformanceRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            task_id: task_id.to_owned(),
            limit,
            authentication: Some(authentication),
            page_before_recorded_at: String::new(),
            page_before_conformance_id: String::new(),
        }
    }

    #[tokio::test]
    async fn records_needs_human_conformance_as_pending_review() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("conformance-pending");
        register_test_key(&database_url, &identity).await;
        let service = EvidenceGrpcService::new(
            EvidenceStore::connect(&database_url)
                .await
                .expect("the gated test database should accept an evidence-store connection"),
        );
        let evidence = service
            .record_evidence(Request::new(record_request(&identity, "task:123", 7)))
            .await
            .expect("evidence should be recorded before conformance")
            .into_inner();
        let conformance = service
            .record_conformance(Request::new(conformance_request(
                &identity,
                "task:123",
                &evidence.evidence_id,
                v1::ConformanceVerdict::NeedsHuman,
                8,
            )))
            .await
            .expect("a signed needs-human result should be recorded")
            .into_inner();
        assert_eq!(
            conformance.review_state,
            v1::ConformanceReviewState::Pending as i32
        );
        assert_eq!(conformance.evaluated_by, identity.node_id);

        let listed = service
            .list_conformance(Request::new(list_conformance_request(
                &identity, "task:123", 10, 9,
            )))
            .await
            .expect("list should return the recorded conformance")
            .into_inner();
        assert_eq!(listed.entries, vec![conformance]);
    }

    #[tokio::test]
    async fn a_tampered_conformance_verdict_is_refused_before_persistence() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("conformance-tamper");
        register_test_key(&database_url, &identity).await;
        let service = EvidenceGrpcService::new(
            EvidenceStore::connect(&database_url)
                .await
                .expect("the gated test database should accept an evidence-store connection"),
        );
        let evidence = service
            .record_evidence(Request::new(record_request(&identity, "task:123", 10)))
            .await
            .expect("evidence should be recorded before conformance")
            .into_inner();
        let mut wire = conformance_request(
            &identity,
            "task:123",
            &evidence.evidence_id,
            v1::ConformanceVerdict::Aligned,
            11,
        );
        wire.verdict = v1::ConformanceVerdict::Violation as i32;

        let refused = service
            .record_conformance(Request::new(wire))
            .await
            .expect_err("a tampered signed verdict must be refused");
        assert_eq!(refused.code(), tonic::Code::Unauthenticated);

        let listed = service
            .list_conformance(Request::new(list_conformance_request(
                &identity, "task:123", 10, 12,
            )))
            .await
            .expect("list should prove the forged conformance was not recorded")
            .into_inner();
        assert!(listed.entries.is_empty());
    }

    #[tokio::test]
    async fn only_the_evidence_producer_may_report_its_conformance() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let producer = TestIdentity::fresh("conformance-producer");
        let mut other_node = TestIdentity::fresh("conformance-other-node");
        other_node.tenant_id = producer.tenant_id.clone();
        other_node.repository_id = producer.repository_id.clone();
        register_test_key(&database_url, &producer).await;
        register_test_key(&database_url, &other_node).await;
        let service = EvidenceGrpcService::new(
            EvidenceStore::connect(&database_url)
                .await
                .expect("the gated test database should accept an evidence-store connection"),
        );
        let evidence = service
            .record_evidence(Request::new(record_request(&producer, "task:123", 13)))
            .await
            .expect("producer evidence should be recorded")
            .into_inner();

        let refused = service
            .record_conformance(Request::new(conformance_request(
                &other_node,
                "task:123",
                &evidence.evidence_id,
                v1::ConformanceVerdict::Aligned,
                14,
            )))
            .await
            .expect_err("another enrolled node must not certify producer evidence");
        assert_eq!(refused.code(), tonic::Code::PermissionDenied);

        let listed = service
            .list_conformance(Request::new(list_conformance_request(
                &producer, "task:123", 10, 15,
            )))
            .await
            .expect("list should prove the unauthorized outcome was not recorded")
            .into_inner();
        assert!(listed.entries.is_empty());
    }

    #[tokio::test]
    async fn retrying_a_signed_conformance_with_the_same_idempotency_key_reuses_the_result() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("conformance-retry");
        register_test_key(&database_url, &identity).await;
        let service = EvidenceGrpcService::new(
            EvidenceStore::connect(&database_url)
                .await
                .expect("the gated test database should accept an evidence-store connection"),
        );
        let evidence = service
            .record_evidence(Request::new(record_request(&identity, "task:123", 16)))
            .await
            .expect("evidence should be recorded before conformance")
            .into_inner();
        let idempotency_key = "conformance:lost-response";
        let first = service
            .record_conformance(Request::new(conformance_request_with_idempotency(
                &identity,
                "task:123",
                &evidence.evidence_id,
                v1::ConformanceVerdict::NeedsHuman,
                idempotency_key,
                17,
            )))
            .await
            .expect("the first conformance write should succeed")
            .into_inner();
        let replay = service
            .record_conformance(Request::new(conformance_request_with_idempotency(
                &identity,
                "task:123",
                &evidence.evidence_id,
                v1::ConformanceVerdict::NeedsHuman,
                idempotency_key,
                18,
            )))
            .await
            .expect("a re-signed retry should return the original conformance")
            .into_inner();

        assert!(!first.idempotent_replay);
        assert!(replay.idempotent_replay);
        assert_eq!(replay.conformance_id, first.conformance_id);

        let listed = service
            .list_conformance(Request::new(list_conformance_request(
                &identity, "task:123", 10, 19,
            )))
            .await
            .expect("list should contain exactly one idempotent outcome")
            .into_inner();
        assert_eq!(listed.entries.len(), 1);
        assert_eq!(listed.entries[0].conformance_id, first.conformance_id);
    }

    #[tokio::test]
    async fn retrying_a_sub_microsecond_check_time_reuses_the_normalized_result() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("conformance-nanoseconds");
        register_test_key(&database_url, &identity).await;
        let service = EvidenceGrpcService::new(
            EvidenceStore::connect(&database_url)
                .await
                .expect("the gated test database should accept an evidence-store connection"),
        );
        let evidence = service
            .record_evidence(Request::new(record_request(&identity, "task:123", 24)))
            .await
            .expect("evidence should be recorded before conformance")
            .into_inner();
        let idempotency_key = "conformance:nanoseconds";
        let first = service
            .record_conformance(Request::new(conformance_request_with_idempotency_and_time(
                &identity,
                "task:123",
                &evidence.evidence_id,
                v1::ConformanceVerdict::NeedsHuman,
                idempotency_key,
                "2026-01-01T00:00:00.123456789Z",
                25,
            )))
            .await
            .expect("the first nanosecond-precision result should succeed")
            .into_inner();
        let replay = service
            .record_conformance(Request::new(conformance_request_with_idempotency_and_time(
                &identity,
                "task:123",
                &evidence.evidence_id,
                v1::ConformanceVerdict::NeedsHuman,
                idempotency_key,
                "2026-01-01T00:00:00.123456789Z",
                26,
            )))
            .await
            .expect("a nanosecond-precision retry should reuse the stored result")
            .into_inner();

        assert!(replay.idempotent_replay);
        assert_eq!(replay.conformance_id, first.conformance_id);
        assert_eq!(replay.reported_checked_at, "2026-01-01T00:00:00.123456Z");
    }

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[44; 32])
    }

    struct TestIdentity {
        signing_key_id: String,
        node_id: String,
        tenant_id: String,
        repository_id: String,
    }

    impl TestIdentity {
        fn fresh(label: &str) -> Self {
            let suffix = crate::test_support::uuid_ish();
            Self {
                signing_key_id: format!("evidence-service-{label}-key-{suffix}"),
                node_id: format!("evidence-service-{label}-node-{suffix}"),
                tenant_id: format!("evidence-service-{label}-tenant-{suffix}"),
                repository_id: format!("evidence-service-{label}-repository-{suffix}"),
            }
        }
    }

    async fn register_test_key(database_url: &str, identity: &TestIdentity) {
        let (mut client, connection) = tokio_postgres::connect(database_url, tokio_postgres::NoTls)
            .await
            .expect("the gated test database should accept a signing-key connection");
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let transaction = client
            .transaction()
            .await
            .expect("a transaction should open for key registration");
        let key = signing_key();
        signing_keys::register(
            &transaction,
            &SigningKeyRecord {
                signing_key_id: identity.signing_key_id.clone(),
                tenant_id: identity.tenant_id.clone(),
                repository_id: identity.repository_id.clone(),
                node_id: identity.node_id.clone(),
                public_key: key.verifying_key().to_bytes().to_vec(),
                public_key_fingerprint: identity.signing_key_id.clone(),
                activated_at: SystemTime::UNIX_EPOCH,
                expires_at: None,
            },
        )
        .await
        .expect("registering the test key should succeed");
        transaction
            .commit()
            .await
            .expect("the registration transaction should commit");
    }

    fn record_request(
        identity: &TestIdentity,
        task_id: &str,
        nonce_byte: u8,
    ) -> v1::RecordEvidenceRequest {
        record_request_with_idempotency(
            identity,
            task_id,
            "commit:0123456789abcdef",
            &format!("evidence:{nonce_byte}"),
            nonce_byte,
        )
    }

    fn record_request_with_source_ref(
        identity: &TestIdentity,
        task_id: &str,
        source_ref: &str,
        nonce_byte: u8,
    ) -> v1::RecordEvidenceRequest {
        record_request_with_idempotency(
            identity,
            task_id,
            source_ref,
            &format!("evidence:{nonce_byte}"),
            nonce_byte,
        )
    }

    fn record_request_with_idempotency(
        identity: &TestIdentity,
        task_id: &str,
        source_ref: &str,
        idempotency_key: &str,
        nonce_byte: u8,
    ) -> v1::RecordEvidenceRequest {
        let key = signing_key();
        let mut authentication = v1::EvidenceAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
            nonce: vec![nonce_byte; 16],
            signature: Vec::new(),
        };
        let observed_at = "2026-01-01T00:00:00Z";
        let content_digest = vec![7; 32];
        let reported_agent_session_id = "session:v1:evidence-test";
        let reported_constitution_version = "constitution:v4";
        let operation = EvidenceOperation::Record {
            task_id,
            evidence_kind: v1::EvidenceKind::Commit as i32,
            source_ref,
            content_digest: &content_digest,
            observed_at,
            reported_agent_session_id,
            idempotency_key,
            reported_constitution_version,
        };
        let bytes = evidence_signature::evidence_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        v1::RecordEvidenceRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            task_id: task_id.to_owned(),
            kind: v1::EvidenceKind::Commit as i32,
            source_ref: source_ref.to_owned(),
            content_digest,
            observed_at: observed_at.to_owned(),
            reported_agent_session_id: reported_agent_session_id.to_owned(),
            authentication: Some(authentication),
            idempotency_key: idempotency_key.to_owned(),
            reported_constitution_version: reported_constitution_version.to_owned(),
        }
    }

    fn list_request(
        identity: &TestIdentity,
        task_id: &str,
        limit: u32,
        nonce_byte: u8,
    ) -> v1::ListEvidenceRequest {
        list_request_with_cursor(identity, task_id, limit, None, nonce_byte)
    }

    fn list_request_with_cursor(
        identity: &TestIdentity,
        task_id: &str,
        limit: u32,
        cursor: Option<(&str, &str)>,
        nonce_byte: u8,
    ) -> v1::ListEvidenceRequest {
        let (page_before_recorded_at, page_before_evidence_id) = cursor.unwrap_or(("", ""));
        let key = signing_key();
        let mut authentication = v1::EvidenceAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
            nonce: vec![nonce_byte; 16],
            signature: Vec::new(),
        };
        let operation = EvidenceOperation::List {
            task_id,
            limit,
            page_before_recorded_at: (!page_before_recorded_at.is_empty())
                .then_some(page_before_recorded_at),
            page_before_evidence_id: (!page_before_evidence_id.is_empty())
                .then_some(page_before_evidence_id),
        };
        let bytes = evidence_signature::evidence_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        v1::ListEvidenceRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            task_id: task_id.to_owned(),
            limit,
            authentication: Some(authentication),
            page_before_recorded_at: page_before_recorded_at.to_owned(),
            page_before_evidence_id: page_before_evidence_id.to_owned(),
        }
    }

    #[tokio::test]
    async fn records_and_lists_signed_evidence_with_authenticated_provenance() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("record-list");
        register_test_key(&database_url, &identity).await;
        let service = EvidenceGrpcService::new(
            EvidenceStore::connect(&database_url)
                .await
                .expect("the gated test database should accept an evidence-store connection"),
        );

        let recorded = service
            .record_evidence(Request::new(record_request(&identity, "task:123", 1)))
            .await
            .expect("a valid signed evidence record should persist")
            .into_inner();
        assert_eq!(recorded.task_id, "task:123");
        assert_eq!(recorded.kind, v1::EvidenceKind::Commit as i32);
        assert_eq!(recorded.content_digest, vec![7; 32]);
        assert_eq!(recorded.recorded_by, identity.node_id);

        let listed = service
            .list_evidence(Request::new(list_request(&identity, "task:123", 1_000, 2)))
            .await
            .expect("a valid signed list request should return evidence")
            .into_inner();
        assert_eq!(listed.effective_limit, MAX_EVIDENCE_LIMIT);
        assert_eq!(listed.entries, vec![recorded]);
    }

    #[tokio::test]
    async fn evidence_list_returns_a_signed_cursor_for_the_next_page() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("evidence-pagination");
        register_test_key(&database_url, &identity).await;
        let service = EvidenceGrpcService::new(
            EvidenceStore::connect(&database_url)
                .await
                .expect("the gated test database should accept an evidence-store connection"),
        );
        for nonce_byte in [20, 21] {
            service
                .record_evidence(Request::new(record_request(
                    &identity, "task:123", nonce_byte,
                )))
                .await
                .expect("evidence should be recorded for pagination");
        }

        let first = service
            .list_evidence(Request::new(list_request(&identity, "task:123", 1, 22)))
            .await
            .expect("first page should be listed")
            .into_inner();
        assert_eq!(first.entries.len(), 1);
        assert!(!first.next_recorded_at.is_empty());
        assert!(!first.next_evidence_id.is_empty());

        let second = service
            .list_evidence(Request::new(list_request_with_cursor(
                &identity,
                "task:123",
                1,
                Some((&first.next_recorded_at, &first.next_evidence_id)),
                23,
            )))
            .await
            .expect("second page should honor the signed cursor")
            .into_inner();
        assert_eq!(second.entries.len(), 1);
        assert_ne!(first.entries[0].evidence_id, second.entries[0].evidence_id);
    }

    #[tokio::test]
    async fn retrying_signed_evidence_with_the_same_idempotency_key_reuses_the_result() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("evidence-retry");
        register_test_key(&database_url, &identity).await;
        let service = EvidenceGrpcService::new(
            EvidenceStore::connect(&database_url)
                .await
                .expect("the gated test database should accept an evidence-store connection"),
        );
        let idempotency_key = "evidence:lost-response";
        let first = service
            .record_evidence(Request::new(record_request_with_idempotency(
                &identity,
                "task:123",
                "commit:0123456789abcdef",
                idempotency_key,
                27,
            )))
            .await
            .expect("the first Evidence write should succeed")
            .into_inner();
        let replay = service
            .record_evidence(Request::new(record_request_with_idempotency(
                &identity,
                "task:123",
                "commit:0123456789abcdef",
                idempotency_key,
                28,
            )))
            .await
            .expect("a re-signed Evidence retry should reuse the original record")
            .into_inner();

        assert!(!first.idempotent_replay);
        assert!(replay.idempotent_replay);
        assert_eq!(replay.evidence_id, first.evidence_id);

        let listed = service
            .list_evidence(Request::new(list_request(&identity, "task:123", 10, 29)))
            .await
            .expect("list should contain exactly one idempotent Evidence record")
            .into_inner();
        assert_eq!(listed.entries.len(), 1);
        assert_eq!(listed.entries[0].evidence_id, first.evidence_id);
    }

    #[tokio::test]
    async fn a_tampered_task_is_refused_before_evidence_is_persisted() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("tampered-task");
        register_test_key(&database_url, &identity).await;
        let service = EvidenceGrpcService::new(
            EvidenceStore::connect(&database_url)
                .await
                .expect("the gated test database should accept an evidence-store connection"),
        );
        let mut wire = record_request(&identity, "task:123", 3);
        wire.task_id = "task:tampered".to_owned();

        let refused = service
            .record_evidence(Request::new(wire))
            .await
            .expect_err("a tampered signed task must be refused");
        assert_eq!(refused.code(), tonic::Code::Unauthenticated);

        let listed = service
            .list_evidence(Request::new(list_request(&identity, "task:123", 10, 4)))
            .await
            .expect("list should prove the forged evidence was not recorded")
            .into_inner();
        assert!(listed.entries.is_empty());
    }

    #[tokio::test]
    async fn a_signed_untyped_reference_is_rejected_before_evidence_is_persisted() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("untyped-reference");
        register_test_key(&database_url, &identity).await;
        let service = EvidenceGrpcService::new(
            EvidenceStore::connect(&database_url)
                .await
                .expect("the gated test database should accept an evidence-store connection"),
        );
        let wire = record_request_with_source_ref(
            &identity,
            "task:123",
            "raw terminal output must not be evidence storage",
            5,
        );

        let refused = service
            .record_evidence(Request::new(wire))
            .await
            .expect_err("a signed untyped reference must be rejected");
        assert_eq!(refused.code(), tonic::Code::InvalidArgument);

        let listed = service
            .list_evidence(Request::new(list_request(&identity, "task:123", 10, 6)))
            .await
            .expect("list should prove invalid evidence was not recorded")
            .into_inner();
        assert!(listed.entries.is_empty());
    }
}

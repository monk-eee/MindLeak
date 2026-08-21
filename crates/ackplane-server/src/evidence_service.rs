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
    EvidenceKind, EvidenceRecord, EvidenceStore, EvidenceStoreError, RecordEvidenceRequest,
};

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
        Ok(authentication.node_id.clone())
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
        EvidenceStoreError::InvalidIdentity => {
            Status::invalid_argument("agent_session_id and recorded_by must be bounded identities")
        }
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

fn to_proto(record: EvidenceRecord) -> Result<v1::EvidenceRecord, String> {
    Ok(v1::EvidenceRecord {
        evidence_id: record.evidence_id,
        tenant_id: record.tenant_id,
        repository_id: record.repository_id,
        task_id: record.task_id,
        kind: proto_kind(record.kind),
        source_ref: record.source_ref,
        content_digest: record.content_digest,
        observed_at: rfc3339(record.observed_at)?,
        agent_session_id: record.agent_session_id,
        recorded_by: record.recorded_by,
        recorded_at: rfc3339(record.recorded_at)?,
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
            agent_session_id: &request.agent_session_id,
        };
        let recorded_by = self
            .authenticate(
                &request.tenant_id,
                &request.repository_id,
                &operation,
                request.authentication.as_ref(),
            )
            .await?;
        let observed_at = parse_rfc3339(&request.observed_at)?;
        let recorded = self
            .store
            .lock()
            .await
            .record(RecordEvidenceRequest {
                tenant_id: request.tenant_id,
                repository_id: request.repository_id,
                task_id: request.task_id,
                kind,
                source_ref: request.source_ref,
                content_digest: request.content_digest,
                observed_at,
                agent_session_id: request.agent_session_id,
                recorded_by,
            })
            .await
            .map_err(store_error)?;
        Ok(Response::new(to_proto(recorded).map_err(Status::internal)?))
    }

    async fn list_evidence(
        &self,
        request: Request<v1::ListEvidenceRequest>,
    ) -> Result<Response<v1::ListEvidenceResult>, Status> {
        let request = request.into_inner();
        let operation = EvidenceOperation::List {
            task_id: &request.task_id,
            limit: request.limit,
        };
        self.authenticate(
            &request.tenant_id,
            &request.repository_id,
            &operation,
            request.authentication.as_ref(),
        )
        .await?;
        let limit = effective_limit(request.limit);
        let entries = self
            .store
            .lock()
            .await
            .list(
                &request.tenant_id,
                &request.repository_id,
                &request.task_id,
                i64::from(limit),
            )
            .await
            .map_err(store_error)?;
        Ok(Response::new(v1::ListEvidenceResult {
            entries: entries
                .into_iter()
                .map(to_proto)
                .collect::<Result<Vec<_>, String>>()
                .map_err(Status::internal)?,
            effective_limit: limit,
        }))
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use ackplane_protocol::v1::evidence_service_server::EvidenceService;

    use super::*;
    use crate::signing_keys::{self, SigningKeyRecord};

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
        record_request_with_source_ref(identity, task_id, "commit:0123456789abcdef", nonce_byte)
    }

    fn record_request_with_source_ref(
        identity: &TestIdentity,
        task_id: &str,
        source_ref: &str,
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
        let agent_session_id = "session:v1:evidence-test";
        let operation = EvidenceOperation::Record {
            task_id,
            evidence_kind: v1::EvidenceKind::Commit as i32,
            source_ref,
            content_digest: &content_digest,
            observed_at,
            agent_session_id,
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
            agent_session_id: agent_session_id.to_owned(),
            authentication: Some(authentication),
        }
    }

    fn list_request(
        identity: &TestIdentity,
        task_id: &str,
        limit: u32,
        nonce_byte: u8,
    ) -> v1::ListEvidenceRequest {
        let key = signing_key();
        let mut authentication = v1::EvidenceAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: time::OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
            nonce: vec![nonce_byte; 16],
            signature: Vec::new(),
        };
        let operation = EvidenceOperation::List { task_id, limit };
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

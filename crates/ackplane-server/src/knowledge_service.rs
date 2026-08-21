//! gRPC transport for Ackplane's knowledge domain (ADR-0106 decision 3).
//!
//! Authenticated the same way `ClaimDelegationService` is (ADR-0108): every
//! mutating RPC verifies a `KnowledgeAuthentication` against the enrolled
//! node's resolved signing key before it reaches the store, mirrored into
//! its own domain (`knowledge_auth`/`knowledge_signature`, its own nonce
//! table) rather than reusing `ClaimOperation`'s claim-shaped fields, which
//! have no equivalent for a knowledge statement.

use std::sync::Arc;
use std::time::SystemTime;

use ackplane_protocol::knowledge_auth::KnowledgeOperation;
use ackplane_protocol::v1;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use crate::knowledge_signature::{self, KnowledgeAuthRefusal};
use crate::knowledge_store::{
    ActiveKnowledge, KnowledgeHistoryEntry as StoreKnowledgeHistoryEntry, KnowledgeReconfirmation,
    KnowledgeStore, KnowledgeStoreError, RecordKnowledgeRequest,
};

const DEFAULT_KNOWLEDGE_HISTORY_LIMIT: u32 = 20;
const MAX_KNOWLEDGE_HISTORY_LIMIT: u32 = 100;

pub struct KnowledgeGrpcService {
    store: Arc<Mutex<KnowledgeStore>>,
}

impl KnowledgeGrpcService {
    pub fn new(store: KnowledgeStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }

    /// Verify a knowledge request's authentication before it reaches the
    /// store (ADR-0108, mirroring `ClaimDelegationService::authenticate`). An
    /// absent, unresolvable, mismatched-binding, not-yet-active, expired,
    /// retired, or revoked key is refused here -- the store methods never
    /// see an unauthenticated caller.
    async fn authenticate(
        &self,
        tenant_id: &str,
        repository_id: &str,
        operation: &KnowledgeOperation<'_>,
        authentication: Option<&v1::KnowledgeAuthentication>,
    ) -> Result<String, Status> {
        let Some(authentication) = authentication else {
            return Err(Status::unauthenticated(
                KnowledgeAuthRefusal::Unsigned.diagnostic(),
            ));
        };
        let binding = crate::signing_keys::EnvelopeBinding {
            signing_key_id: &authentication.signing_key_id,
            tenant_id,
            repository_id,
            producer_id: &authentication.node_id,
            accepted_at: SystemTime::now(),
        };
        let resolution = self
            .store
            .lock()
            .await
            .resolve_signing_key(&binding)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;

        knowledge_signature::verify(
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

        // Only after a genuine signature is confirmed: a forged request must
        // never be able to burn a legitimate nonce out from under its owner.
        let fresh = self
            .store
            .lock()
            .await
            .consume_knowledge_nonce(
                &authentication.signing_key_id,
                &authentication.nonce,
                SystemTime::now(),
            )
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        if !fresh {
            return Err(Status::unauthenticated(
                KnowledgeAuthRefusal::Replayed.diagnostic(),
            ));
        }
        Ok(authentication.node_id.clone())
    }
}

fn store_error(error: KnowledgeStoreError) -> Status {
    match error {
        KnowledgeStoreError::InvalidHalfLife => {
            Status::invalid_argument("half_life_hours must be greater than zero")
        }
        KnowledgeStoreError::EmptyContent => Status::invalid_argument("content must not be empty"),
        KnowledgeStoreError::EmptyReconfirmationEvidence => {
            Status::invalid_argument("reconfirmation evidence_ref must not be empty")
        }
        KnowledgeStoreError::Database(error) => Status::internal(error.to_string()),
    }
}

fn to_proto_entry(entry: ActiveKnowledge) -> Result<v1::ActiveKnowledgeEntry, String> {
    Ok(v1::ActiveKnowledgeEntry {
        knowledge_id: entry.knowledge_id,
        content: entry.content,
        source_ref: entry.source_ref.unwrap_or_default(),
        effective_weight: entry.effective_weight,
        confirmed_at: rfc3339(entry.confirmed_at)?,
        recorded_by: entry.recorded_by.unwrap_or_default(),
        last_reconfirmed_at: entry
            .last_reconfirmed_at
            .map(rfc3339)
            .transpose()?
            .unwrap_or_default(),
        last_reconfirmed_by: entry.last_reconfirmed_by.unwrap_or_default(),
        last_reconfirmation_evidence_ref: entry
            .last_reconfirmation_evidence_ref
            .unwrap_or_default(),
    })
}

fn to_proto_history_entry(
    entry: StoreKnowledgeHistoryEntry,
) -> Result<v1::KnowledgeHistoryEntry, String> {
    Ok(v1::KnowledgeHistoryEntry {
        knowledge_id: entry.knowledge_id,
        content: entry.content,
        source_ref: entry.source_ref.unwrap_or_default(),
        recorded_by: entry.recorded_by.unwrap_or_default(),
        confirmed_at: rfc3339(entry.confirmed_at)?,
        retired_at: entry
            .retired_at
            .map(rfc3339)
            .transpose()?
            .unwrap_or_default(),
        retired_reason: entry.retired_reason.unwrap_or_default(),
        retired_by: entry.retired_by.unwrap_or_default(),
        last_reconfirmed_at: entry
            .last_reconfirmed_at
            .map(rfc3339)
            .transpose()?
            .unwrap_or_default(),
        last_reconfirmed_by: entry.last_reconfirmed_by.unwrap_or_default(),
        last_reconfirmation_evidence_ref: entry
            .last_reconfirmation_evidence_ref
            .unwrap_or_default(),
    })
}

fn to_proto_reconfirmation(
    reconfirmation: Option<KnowledgeReconfirmation>,
) -> Result<v1::ReconfirmKnowledgeResult, String> {
    let Some(reconfirmation) = reconfirmation else {
        return Ok(v1::ReconfirmKnowledgeResult {
            reconfirmed: false,
            reconfirmation_id: String::new(),
            evidence_ref: String::new(),
            reconfirmed_by: String::new(),
            reconfirmed_at: String::new(),
        });
    };
    Ok(v1::ReconfirmKnowledgeResult {
        reconfirmed: true,
        reconfirmation_id: reconfirmation.reconfirmation_id,
        evidence_ref: reconfirmation.evidence_ref,
        reconfirmed_by: reconfirmation.reconfirmed_by,
        reconfirmed_at: rfc3339(reconfirmation.reconfirmed_at)?,
    })
}

fn rfc3339(timestamp: std::time::SystemTime) -> Result<String, String> {
    time::OffsetDateTime::from(timestamp)
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| format!("could not format a knowledge timestamp: {error}"))
}

fn history_limit(requested: u32) -> u32 {
    if requested == 0 {
        DEFAULT_KNOWLEDGE_HISTORY_LIMIT
    } else {
        requested.min(MAX_KNOWLEDGE_HISTORY_LIMIT)
    }
}

#[tonic::async_trait]
impl v1::knowledge_service_server::KnowledgeService for KnowledgeGrpcService {
    async fn record_knowledge(
        &self,
        request: Request<v1::RecordKnowledgeRequest>,
    ) -> Result<Response<v1::KnowledgeRecord>, Status> {
        let request = request.into_inner();
        let operation = KnowledgeOperation::Record {
            content: &request.content,
            source_ref: (!request.source_ref.is_empty()).then_some(request.source_ref.as_str()),
            half_life_hours: request.half_life_hours,
            embedding_model: (!request.embedding_model.is_empty())
                .then_some(request.embedding_model.as_str()),
        };
        let recorded_by = self
            .authenticate(
                &request.tenant_id,
                &request.repository_id,
                &operation,
                request.authentication.as_ref(),
            )
            .await?;
        let embedding = if request.embedding.is_empty() {
            None
        } else {
            Some((request.embedding_model, request.embedding))
        };
        let recorded = self
            .store
            .lock()
            .await
            .record(RecordKnowledgeRequest {
                tenant_id: request.tenant_id,
                repository_id: request.repository_id,
                content: request.content,
                source_ref: (!request.source_ref.is_empty()).then_some(request.source_ref),
                recorded_by: Some(recorded_by),
                half_life_hours: request.half_life_hours,
                embedding,
            })
            .await
            .map_err(store_error)?;
        Ok(Response::new(v1::KnowledgeRecord {
            knowledge_id: recorded.knowledge_id,
            tenant_id: recorded.tenant_id,
            repository_id: recorded.repository_id,
            content: recorded.content,
            source_ref: recorded.source_ref.unwrap_or_default(),
            half_life_hours: recorded.half_life_hours,
            confirmed_at: rfc3339(recorded.confirmed_at).map_err(Status::internal)?,
            recorded_by: recorded.recorded_by.unwrap_or_default(),
        }))
    }

    async fn recall_knowledge(
        &self,
        request: Request<v1::RecallKnowledgeRequest>,
    ) -> Result<Response<v1::RecallKnowledgeResult>, Status> {
        let request = request.into_inner();
        let limit = if request.limit == 0 {
            20
        } else {
            request.limit as i64
        };
        let operation = KnowledgeOperation::Recall {
            query_embedding_present: !request.query_embedding.is_empty(),
            limit: request.limit,
        };
        self.authenticate(
            &request.tenant_id,
            &request.repository_id,
            &operation,
            request.authentication.as_ref(),
        )
        .await?;
        let embedding = if request.query_embedding.is_empty() {
            None
        } else {
            Some((request.embedding_model.as_str(), request.query_embedding))
        };
        let recalled = self
            .store
            .lock()
            .await
            .recall(&request.tenant_id, &request.repository_id, embedding, limit)
            .await
            .map_err(store_error)?;
        Ok(Response::new(v1::RecallKnowledgeResult {
            entries: recalled
                .entries
                .into_iter()
                .map(to_proto_entry)
                .collect::<Result<Vec<_>, String>>()
                .map_err(Status::internal)?,
            ranked_by_similarity: recalled.ranked_by_similarity,
        }))
    }

    async fn get_knowledge_history(
        &self,
        request: Request<v1::KnowledgeHistoryRequest>,
    ) -> Result<Response<v1::KnowledgeHistoryResult>, Status> {
        let request = request.into_inner();
        let operation = KnowledgeOperation::History {
            limit: request.limit,
        };
        self.authenticate(
            &request.tenant_id,
            &request.repository_id,
            &operation,
            request.authentication.as_ref(),
        )
        .await?;
        let effective_limit = history_limit(request.limit);
        let entries = self
            .store
            .lock()
            .await
            .history(
                &request.tenant_id,
                &request.repository_id,
                i64::from(effective_limit),
            )
            .await
            .map_err(store_error)?;
        Ok(Response::new(v1::KnowledgeHistoryResult {
            entries: entries
                .into_iter()
                .map(to_proto_history_entry)
                .collect::<Result<Vec<_>, String>>()
                .map_err(Status::internal)?,
            effective_limit,
        }))
    }

    async fn reconfirm_knowledge(
        &self,
        request: Request<v1::ReconfirmKnowledgeRequest>,
    ) -> Result<Response<v1::ReconfirmKnowledgeResult>, Status> {
        let request = request.into_inner();
        let operation = KnowledgeOperation::Reconfirm {
            knowledge_id: &request.knowledge_id,
            evidence_ref: &request.evidence_ref,
        };
        let reconfirmed_by = self
            .authenticate(
                &request.tenant_id,
                &request.repository_id,
                &operation,
                request.authentication.as_ref(),
            )
            .await?;
        let reconfirmation = self
            .store
            .lock()
            .await
            .reconfirm(
                &request.tenant_id,
                &request.repository_id,
                &request.knowledge_id,
                &request.evidence_ref,
                &reconfirmed_by,
                SystemTime::now(),
            )
            .await
            .map_err(store_error)?;
        Ok(Response::new(
            to_proto_reconfirmation(reconfirmation).map_err(Status::internal)?,
        ))
    }

    async fn retire_knowledge(
        &self,
        request: Request<v1::RetireKnowledgeRequest>,
    ) -> Result<Response<v1::RetireKnowledgeResult>, Status> {
        let request = request.into_inner();
        let operation = KnowledgeOperation::Retire {
            knowledge_id: &request.knowledge_id,
            reason: &request.reason,
        };
        let retired_by = self
            .authenticate(
                &request.tenant_id,
                &request.repository_id,
                &operation,
                request.authentication.as_ref(),
            )
            .await?;
        let retired = self
            .store
            .lock()
            .await
            .retire(
                &request.tenant_id,
                &request.repository_id,
                &request.knowledge_id,
                &request.reason,
                &retired_by,
            )
            .await
            .map_err(store_error)?;
        Ok(Response::new(v1::RetireKnowledgeResult { retired }))
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use ackplane_protocol::v1::knowledge_service_server::KnowledgeService;

    use super::*;
    use crate::signing_keys::{self, SigningKeyRecord};

    /// Deterministic key material across every test -- matching
    /// `claim_service.rs`'s own fixture: a fixed key is fine because each
    /// test registers it under its own freshly generated identity.
    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[34; 32])
    }

    /// One test's fully-isolated tenant/repository/node/key identity, so
    /// tests in the same binary never share a row and never depend on
    /// registration order.
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
                signing_key_id: format!("knowledge-service-{label}-key-{suffix}"),
                node_id: format!("knowledge-service-{label}-node-{suffix}"),
                tenant_id: format!("knowledge-service-{label}-tenant-{suffix}"),
                repository_id: format!("knowledge-service-{label}-repository-{suffix}"),
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

    /// A validly-signed `RecordKnowledgeRequest`, `signed_at` pinned to "now"
    /// and `nonce` distinguished by the caller so two requests in the same
    /// test can be deliberately identical or distinct.
    fn authenticated_record_request(
        identity: &TestIdentity,
        content: &str,
        nonce_byte: u8,
    ) -> v1::RecordKnowledgeRequest {
        authenticated_record_request_with_source_ref(identity, content, "", nonce_byte)
    }

    fn authenticated_record_request_with_source_ref(
        identity: &TestIdentity,
        content: &str,
        source_ref: &str,
        nonce_byte: u8,
    ) -> v1::RecordKnowledgeRequest {
        let key = signing_key();
        let half_life_hours = 720.0;
        let mut authentication = v1::KnowledgeAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            nonce: vec![nonce_byte; 16],
            signature: Vec::new(),
        };
        let operation = KnowledgeOperation::Record {
            content,
            source_ref: (!source_ref.is_empty()).then_some(source_ref),
            half_life_hours,
            embedding_model: None,
        };
        let bytes = knowledge_signature::knowledge_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        v1::RecordKnowledgeRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            content: content.to_owned(),
            source_ref: source_ref.to_owned(),
            half_life_hours,
            embedding_model: String::new(),
            embedding: Vec::new(),
            authentication: Some(authentication),
        }
    }

    fn authenticated_retire_request(
        identity: &TestIdentity,
        knowledge_id: &str,
        reason: &str,
        retired_by: &str,
        nonce_byte: u8,
    ) -> v1::RetireKnowledgeRequest {
        let key = signing_key();
        let mut authentication = v1::KnowledgeAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            nonce: vec![nonce_byte; 16],
            signature: Vec::new(),
        };
        let operation = KnowledgeOperation::Retire {
            knowledge_id,
            reason,
        };
        let bytes = knowledge_signature::knowledge_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        v1::RetireKnowledgeRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            knowledge_id: knowledge_id.to_owned(),
            reason: reason.to_owned(),
            retired_by: retired_by.to_owned(),
            authentication: Some(authentication),
        }
    }

    fn authenticated_history_request(
        identity: &TestIdentity,
        limit: u32,
        nonce_byte: u8,
    ) -> v1::KnowledgeHistoryRequest {
        let key = signing_key();
        let mut authentication = v1::KnowledgeAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            nonce: vec![nonce_byte; 16],
            signature: Vec::new(),
        };
        let operation = KnowledgeOperation::History { limit };
        let bytes = knowledge_signature::knowledge_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        v1::KnowledgeHistoryRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            limit,
            authentication: Some(authentication),
        }
    }

    fn authenticated_reconfirm_request(
        identity: &TestIdentity,
        knowledge_id: &str,
        evidence_ref: &str,
        nonce_byte: u8,
    ) -> v1::ReconfirmKnowledgeRequest {
        let key = signing_key();
        let mut authentication = v1::KnowledgeAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            nonce: vec![nonce_byte; 16],
            signature: Vec::new(),
        };
        let operation = KnowledgeOperation::Reconfirm {
            knowledge_id,
            evidence_ref,
        };
        let bytes = knowledge_signature::knowledge_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        v1::ReconfirmKnowledgeRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            knowledge_id: knowledge_id.to_owned(),
            evidence_ref: evidence_ref.to_owned(),
            authentication: Some(authentication),
        }
    }

    /// Proves `authenticate` actually wires nonce consumption into the RPC
    /// path: the identical wire request granted the first time is refused
    /// the second time on the same (signing_key_id, nonce) pair. Without
    /// this, a captured `record_knowledge` request stays replayable forever.
    #[tokio::test]
    async fn an_identical_request_is_granted_once_then_refused_as_replayed() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("replay");
        register_test_key(&database_url, &identity).await;
        let store = KnowledgeStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a knowledge-store connection");
        let service = KnowledgeGrpcService::new(store);
        let wire = authenticated_record_request(&identity, "a replay-tested lesson", 91);

        let recorded = service
            .record_knowledge(Request::new(wire.clone()))
            .await
            .expect("the first, fresh request must be authenticated and recorded");
        let recorded = recorded.into_inner();
        assert_eq!(recorded.content, "a replay-tested lesson");
        assert_eq!(recorded.recorded_by, identity.node_id);

        let replayed = service
            .record_knowledge(Request::new(wire))
            .await
            .expect_err("the identical (signing_key_id, nonce) pair must be refused");
        assert_eq!(replayed.code(), tonic::Code::Unauthenticated);
        assert!(
            replayed.message().contains("already been used"),
            "unexpected diagnostic: {}",
            replayed.message()
        );
    }

    /// A `signed_at` far outside the accepted clock-skew window is refused
    /// before the request ever reaches the store -- freshness protects a
    /// captured signature from staying usable indefinitely, independent of
    /// whether its nonce has been seen before.
    #[tokio::test]
    async fn a_stale_signed_at_is_refused_before_the_store_runs() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("stale");
        register_test_key(&database_url, &identity).await;
        let store = KnowledgeStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a knowledge-store connection");
        let service = KnowledgeGrpcService::new(store);
        let mut wire = authenticated_record_request(&identity, "a stale-tested lesson", 92);
        // Re-sign over a `signed_at` far outside the skew window -- the
        // signature must cover the stale timestamp, or this would only prove
        // the diagnostic string exists, not that verification used it.
        let key = signing_key();
        let mut authentication = wire.authentication.take().unwrap();
        authentication.signed_at = "2020-01-01T00:00:00Z".to_owned();
        let operation = KnowledgeOperation::Record {
            content: &wire.content,
            source_ref: (!wire.source_ref.is_empty()).then_some(wire.source_ref.as_str()),
            half_life_hours: wire.half_life_hours,
            embedding_model: None,
        };
        let bytes = knowledge_signature::knowledge_signing_bytes(
            &wire.tenant_id,
            &wire.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        wire.authentication = Some(authentication);

        let refused = service
            .record_knowledge(Request::new(wire))
            .await
            .expect_err("a signed_at far outside the skew window must be refused");
        assert_eq!(refused.code(), tonic::Code::Unauthenticated);
        assert!(
            refused.message().contains("clock-skew"),
            "unexpected diagnostic: {}",
            refused.message()
        );
    }

    /// An unsigned request (no `KnowledgeAuthentication` at all) is refused
    /// before it ever reaches the store -- the previously-unauthenticated
    /// shape this ADR closes must not still work by omission.
    #[tokio::test]
    async fn a_request_with_no_authentication_is_refused() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("unsigned");
        let store = KnowledgeStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a knowledge-store connection");
        let service = KnowledgeGrpcService::new(store);
        let mut wire = authenticated_record_request(&identity, "an unsigned lesson", 93);
        wire.authentication = None;

        let refused = service
            .record_knowledge(Request::new(wire))
            .await
            .expect_err("a request with no authentication must be refused");
        assert_eq!(refused.code(), tonic::Code::Unauthenticated);
    }

    /// Regression: `source_ref` was not signed, so an in-flight mutation
    /// could falsely attribute a statement to unrelated evidence. Binding it
    /// into `KnowledgeOperation::Record` refuses the tampered request before
    /// the store records any knowledge.
    #[tokio::test]
    async fn a_tampered_source_reference_is_refused_before_the_store_runs() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("source-ref-tampering");
        register_test_key(&database_url, &identity).await;
        let service = KnowledgeGrpcService::new(
            KnowledgeStore::connect(&database_url)
                .await
                .expect("the gated test database should accept a knowledge-store connection"),
        );
        let mut wire = authenticated_record_request_with_source_ref(
            &identity,
            "a source-bound lesson",
            "evidence:verified",
            99,
        );
        wire.source_ref = "evidence:tampered".to_owned();

        let refused = service
            .record_knowledge(Request::new(wire))
            .await
            .expect_err("tampering with a signed source reference must be refused");
        assert_eq!(refused.code(), tonic::Code::Unauthenticated);

        let history = KnowledgeStore::connect(&database_url)
            .await
            .expect("a verifier connection should open")
            .history(&identity.tenant_id, &identity.repository_id, 10)
            .await
            .expect("history should prove no statement was recorded");
        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn retirement_attributes_the_authenticated_node_not_the_request_label() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("retirement-attribution");
        register_test_key(&database_url, &identity).await;
        let service = KnowledgeGrpcService::new(
            KnowledgeStore::connect(&database_url)
                .await
                .expect("the gated test database should accept a knowledge-store connection"),
        );
        let recorded = service
            .record_knowledge(Request::new(authenticated_record_request(
                &identity,
                "an attribution-tested lesson",
                94,
            )))
            .await
            .expect("a valid request should record knowledge")
            .into_inner();

        service
            .retire_knowledge(Request::new(authenticated_retire_request(
                &identity,
                &recorded.knowledge_id,
                "superseded by evidence",
                "claimed-by-someone-else",
                95,
            )))
            .await
            .expect("a valid request should retire knowledge");

        let history = KnowledgeStore::connect(&database_url)
            .await
            .expect("a verifier connection should open")
            .history(&identity.tenant_id, &identity.repository_id, 10)
            .await
            .expect("history should include the retired statement");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].knowledge_id, recorded.knowledge_id);
        assert_eq!(
            history[0].retired_by.as_deref(),
            Some(identity.node_id.as_str())
        );
        assert_ne!(
            history[0].retired_by.as_deref(),
            Some("claimed-by-someone-else")
        );
    }

    #[tokio::test]
    async fn history_returns_retirement_provenance_and_its_bounded_effective_limit() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("history");
        register_test_key(&database_url, &identity).await;
        let service = KnowledgeGrpcService::new(
            KnowledgeStore::connect(&database_url)
                .await
                .expect("the gated test database should accept a knowledge-store connection"),
        );
        let recorded = service
            .record_knowledge(Request::new(authenticated_record_request(
                &identity,
                "a lifecycle-visible lesson",
                96,
            )))
            .await
            .expect("a valid request should record knowledge")
            .into_inner();
        service
            .retire_knowledge(Request::new(authenticated_retire_request(
                &identity,
                &recorded.knowledge_id,
                "superseded by verified evidence",
                "untrusted-label",
                97,
            )))
            .await
            .expect("a valid request should retire knowledge");

        let response = service
            .get_knowledge_history(Request::new(authenticated_history_request(
                &identity, 1_000, 98,
            )))
            .await
            .expect("a valid request should return history")
            .into_inner();

        assert_eq!(response.effective_limit, MAX_KNOWLEDGE_HISTORY_LIMIT);
        assert_eq!(response.entries.len(), 1);
        let entry = &response.entries[0];
        assert_eq!(entry.knowledge_id, recorded.knowledge_id);
        assert_eq!(entry.content, "a lifecycle-visible lesson");
        assert_eq!(entry.source_ref, "");
        assert_eq!(entry.recorded_by, identity.node_id);
        assert!(time::OffsetDateTime::parse(
            &entry.confirmed_at,
            &time::format_description::well_known::Rfc3339
        )
        .is_ok());
        assert!(time::OffsetDateTime::parse(
            &entry.retired_at,
            &time::format_description::well_known::Rfc3339
        )
        .is_ok());
        assert_eq!(entry.retired_reason, "superseded by verified evidence");
        assert_eq!(entry.retired_by, identity.node_id);
    }

    #[tokio::test]
    async fn reconfirmation_records_authenticated_evidence_and_surfaces_it_in_history() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("reconfirm");
        register_test_key(&database_url, &identity).await;
        let service = KnowledgeGrpcService::new(
            KnowledgeStore::connect(&database_url)
                .await
                .expect("the gated test database should accept a knowledge-store connection"),
        );
        let recorded = service
            .record_knowledge(Request::new(authenticated_record_request(
                &identity,
                "a revalidation-tested lesson",
                100,
            )))
            .await
            .expect("a valid request should record knowledge")
            .into_inner();

        let reconfirmed = service
            .reconfirm_knowledge(Request::new(authenticated_reconfirm_request(
                &identity,
                &recorded.knowledge_id,
                "evidence:corroborated",
                101,
            )))
            .await
            .expect("a valid request should reconfirm knowledge")
            .into_inner();
        assert!(reconfirmed.reconfirmed);
        assert!(!reconfirmed.reconfirmation_id.is_empty());
        assert_eq!(reconfirmed.evidence_ref, "evidence:corroborated");
        assert_eq!(reconfirmed.reconfirmed_by, identity.node_id);
        assert!(time::OffsetDateTime::parse(
            &reconfirmed.reconfirmed_at,
            &time::format_description::well_known::Rfc3339
        )
        .is_ok());

        let history = service
            .get_knowledge_history(Request::new(authenticated_history_request(
                &identity, 10, 102,
            )))
            .await
            .expect("history should expose revalidation state")
            .into_inner();
        assert_eq!(history.entries.len(), 1);
        let entry = &history.entries[0];
        assert_eq!(entry.knowledge_id, recorded.knowledge_id);
        assert_eq!(entry.last_reconfirmed_by, identity.node_id);
        assert_eq!(
            entry.last_reconfirmation_evidence_ref,
            "evidence:corroborated"
        );
        assert!(time::OffsetDateTime::parse(
            &entry.last_reconfirmed_at,
            &time::format_description::well_known::Rfc3339
        )
        .is_ok());
    }

    #[tokio::test]
    async fn a_tampered_reconfirmation_evidence_reference_is_refused() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("reconfirm-tampering");
        register_test_key(&database_url, &identity).await;
        let service = KnowledgeGrpcService::new(
            KnowledgeStore::connect(&database_url)
                .await
                .expect("the gated test database should accept a knowledge-store connection"),
        );
        let recorded = service
            .record_knowledge(Request::new(authenticated_record_request(
                &identity,
                "a tamper-resistant revalidation lesson",
                103,
            )))
            .await
            .expect("a valid request should record knowledge")
            .into_inner();
        let mut wire = authenticated_reconfirm_request(
            &identity,
            &recorded.knowledge_id,
            "evidence:verified",
            104,
        );
        wire.evidence_ref = "evidence:tampered".to_owned();

        let refused = service
            .reconfirm_knowledge(Request::new(wire))
            .await
            .expect_err("tampering with signed reconfirmation evidence must be refused");
        assert_eq!(refused.code(), tonic::Code::Unauthenticated);

        let history = service
            .get_knowledge_history(Request::new(authenticated_history_request(
                &identity, 10, 105,
            )))
            .await
            .expect("history should prove the forged reconfirmation did not persist")
            .into_inner();
        assert_eq!(history.entries.len(), 1);
        assert_eq!(history.entries[0].last_reconfirmed_at, "");
        assert_eq!(history.entries[0].last_reconfirmed_by, "");
        assert_eq!(history.entries[0].last_reconfirmation_evidence_ref, "");
    }

    #[tokio::test]
    async fn reconfirmation_rejects_an_empty_corroborating_evidence_reference() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("reconfirm-empty-evidence");
        register_test_key(&database_url, &identity).await;
        let service = KnowledgeGrpcService::new(
            KnowledgeStore::connect(&database_url)
                .await
                .expect("the gated test database should accept a knowledge-store connection"),
        );
        let recorded = service
            .record_knowledge(Request::new(authenticated_record_request(
                &identity,
                "a reconfirmation validation lesson",
                106,
            )))
            .await
            .expect("a valid request should record knowledge")
            .into_inner();

        let refused = service
            .reconfirm_knowledge(Request::new(authenticated_reconfirm_request(
                &identity,
                &recorded.knowledge_id,
                "",
                107,
            )))
            .await
            .expect_err("reconfirmation without evidence must be refused");
        assert_eq!(refused.code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn a_retired_lesson_cannot_acquire_a_reconfirmation_event() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("reconfirm-retired");
        register_test_key(&database_url, &identity).await;
        let service = KnowledgeGrpcService::new(
            KnowledgeStore::connect(&database_url)
                .await
                .expect("the gated test database should accept a knowledge-store connection"),
        );
        let recorded = service
            .record_knowledge(Request::new(authenticated_record_request(
                &identity,
                "a retired reconfirmation lesson",
                108,
            )))
            .await
            .expect("a valid request should record knowledge")
            .into_inner();
        service
            .retire_knowledge(Request::new(authenticated_retire_request(
                &identity,
                &recorded.knowledge_id,
                "withdrawn by evidence",
                "untrusted-label",
                109,
            )))
            .await
            .expect("a valid request should retire knowledge");

        let result = service
            .reconfirm_knowledge(Request::new(authenticated_reconfirm_request(
                &identity,
                &recorded.knowledge_id,
                "evidence:too-late",
                110,
            )))
            .await
            .expect("retired knowledge should return an honest no-op")
            .into_inner();
        assert!(!result.reconfirmed);
        assert_eq!(result.reconfirmation_id, "");

        let history = service
            .get_knowledge_history(Request::new(authenticated_history_request(
                &identity, 10, 111,
            )))
            .await
            .expect("history should show no event was created for the retired lesson")
            .into_inner();
        assert_eq!(history.entries.len(), 1);
        let entry = &history.entries[0];
        assert_eq!(entry.knowledge_id, recorded.knowledge_id);
        assert_ne!(entry.retired_at, "");
        assert_eq!(entry.last_reconfirmed_at, "");
        assert_eq!(entry.last_reconfirmed_by, "");
        assert_eq!(entry.last_reconfirmation_evidence_ref, "");
    }
}

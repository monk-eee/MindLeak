//! gRPC transport for Ackplane's read-only constitution domain (ADR-0106
//! decision 3).
//!
//! Authenticated the same way `KnowledgeGrpcService` is: every RPC verifies a
//! `ConstitutionAuthentication` against the enrolled node's resolved signing
//! key before it reaches the store, mirrored into its own domain
//! (`constitution_auth`/`constitution_signature`, its own nonce table)
//! rather than reusing another domain's operation-shaped fields.

use std::sync::Arc;
use std::time::SystemTime;

use ackplane_protocol::constitution_auth::ConstitutionOperation;
use ackplane_protocol::v1;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use crate::constitution_signature::{self, ConstitutionAuthRefusal};
use crate::constitution_store::{
    ClauseSnapshot, ConstitutionStore, ConstitutionStoreError, PublishConstitutionRequest,
    RecordConstitutionPublicationRequest,
};

pub struct ConstitutionGrpcService {
    store: Arc<Mutex<ConstitutionStore>>,
}

impl ConstitutionGrpcService {
    pub fn new(store: ConstitutionStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }

    /// Verify a constitution request's authentication before it reaches the
    /// store, mirroring `KnowledgeGrpcService::authenticate`. An absent,
    /// unresolvable, mismatched-binding, not-yet-active, expired, retired, or
    /// revoked key is refused here -- the store methods never see an
    /// unauthenticated caller.
    async fn authenticate(
        &self,
        tenant_id: &str,
        repository_id: &str,
        operation: &ConstitutionOperation<'_>,
        authentication: Option<&v1::ConstitutionAuthentication>,
    ) -> Result<(), Status> {
        let Some(authentication) = authentication else {
            return Err(Status::unauthenticated(
                ConstitutionAuthRefusal::Unsigned.diagnostic(),
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

        constitution_signature::verify(
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
            .consume_constitution_nonce(
                &authentication.signing_key_id,
                &authentication.nonce,
                SystemTime::now(),
            )
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        if !fresh {
            return Err(Status::unauthenticated(
                ConstitutionAuthRefusal::Replayed.diagnostic(),
            ));
        }
        Ok(())
    }
}

fn store_error(error: ConstitutionStoreError) -> Status {
    match error {
        ConstitutionStoreError::EmptyVersionId => {
            Status::invalid_argument("version_id must not be empty")
        }
        ConstitutionStoreError::EmptySchemaVersion => {
            Status::invalid_argument("schema_version must not be empty")
        }
        ConstitutionStoreError::EmptyStatus => Status::invalid_argument("status must not be empty"),
        ConstitutionStoreError::PublicationImmutabilityViolation { version_id } => {
            Status::failed_precondition(format!(
                "publication {version_id} is already recorded with different content"
            ))
        }
        ConstitutionStoreError::CorruptPublicationPayload { version_id, detail } => {
            Status::internal(format!(
                "stored publication {version_id} is unreadable: {detail}"
            ))
        }
        ConstitutionStoreError::Database(error) => Status::internal(error.to_string()),
    }
}

fn to_proto_clause(clause: ClauseSnapshot) -> v1::ConstitutionClause {
    v1::ConstitutionClause {
        id: clause.id,
        slug: clause.slug,
        kind: clause.kind,
        title: clause.title,
        statement: clause.statement,
        status: clause.status,
        consequence: clause.consequence.unwrap_or_default(),
        scope: clause.scope.unwrap_or_default(),
        rationale: clause.rationale.unwrap_or_default(),
    }
}

fn from_proto_clause(clause: v1::ConstitutionClause) -> ClauseSnapshot {
    ClauseSnapshot {
        id: clause.id,
        slug: clause.slug,
        kind: clause.kind,
        title: clause.title,
        statement: clause.statement,
        status: clause.status,
        consequence: (!clause.consequence.is_empty()).then_some(clause.consequence),
        scope: (!clause.scope.is_empty()).then_some(clause.scope),
        rationale: (!clause.rationale.is_empty()).then_some(clause.rationale),
    }
}

fn rfc3339(timestamp: std::time::SystemTime) -> Result<String, String> {
    time::OffsetDateTime::from(timestamp)
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| format!("could not format a constitution timestamp: {error}"))
}

#[tonic::async_trait]
impl v1::constitution_service_server::ConstitutionService for ConstitutionGrpcService {
    async fn publish_constitution_snapshot(
        &self,
        request: Request<v1::PublishConstitutionSnapshotRequest>,
    ) -> Result<Response<v1::PublishConstitutionSnapshotResult>, Status> {
        let request = request.into_inner();
        let operation = ConstitutionOperation::Publish {
            version_id: &request.version_id,
            version: request.version,
            status: &request.status,
            clause_count: request.clauses.len() as u32,
        };
        self.authenticate(
            &request.tenant_id,
            &request.repository_id,
            &operation,
            request.authentication.as_ref(),
        )
        .await?;
        let clauses: Vec<ClauseSnapshot> =
            request.clauses.into_iter().map(from_proto_clause).collect();
        let published_at = SystemTime::now();
        // ADR-0121 decision 1/2: record the immutable publication FIRST. An
        // immutability violation (this version_id already recorded under
        // different content) must refuse before the mutable active snapshot
        // changes, not after -- so a rejected republish never silently moves
        // the active pointer.
        self.store
            .lock()
            .await
            .record_publication(RecordConstitutionPublicationRequest {
                tenant_id: request.tenant_id.clone(),
                repository_id: request.repository_id.clone(),
                version_id: request.version_id.clone(),
                schema_version: request.schema_version,
                status: request.status.clone(),
                clauses: clauses.clone(),
                source_reference: (!request.source_ref.is_empty()).then_some(request.source_ref),
                source_digest: (!request.content_digest.is_empty())
                    .then_some(request.content_digest),
                published_at,
            })
            .await
            .map_err(store_error)?;
        self.store
            .lock()
            .await
            .publish(PublishConstitutionRequest {
                tenant_id: request.tenant_id,
                repository_id: request.repository_id,
                version_id: request.version_id,
                version: request.version as i64,
                status: request.status,
                clauses,
            })
            .await
            .map_err(store_error)?;
        Ok(Response::new(v1::PublishConstitutionSnapshotResult {
            published: true,
        }))
    }

    async fn get_active_constitution(
        &self,
        request: Request<v1::GetActiveConstitutionRequest>,
    ) -> Result<Response<v1::GetActiveConstitutionResult>, Status> {
        let request = request.into_inner();
        let operation = ConstitutionOperation::GetActive;
        self.authenticate(
            &request.tenant_id,
            &request.repository_id,
            &operation,
            request.authentication.as_ref(),
        )
        .await?;
        let active = self
            .store
            .lock()
            .await
            .get_active(&request.tenant_id, &request.repository_id)
            .await
            .map_err(store_error)?;
        match active {
            Some(active) => Ok(Response::new(v1::GetActiveConstitutionResult {
                found: true,
                version_id: active.version_id,
                version: active.version as u32,
                status: active.status,
                clauses: active.clauses.into_iter().map(to_proto_clause).collect(),
                published_at: rfc3339(active.published_at).map_err(Status::internal)?,
            })),
            None => Ok(Response::new(v1::GetActiveConstitutionResult {
                found: false,
                version_id: String::new(),
                version: 0,
                status: String::new(),
                clauses: Vec::new(),
                published_at: String::new(),
            })),
        }
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use ackplane_protocol::v1::constitution_service_server::ConstitutionService;

    use super::*;
    use crate::signing_keys::{self, SigningKeyRecord};

    /// Deterministic key material across every test -- matching
    /// `knowledge_service.rs`'s own fixture: a fixed key is fine because each
    /// test registers it under its own freshly generated identity.
    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[41; 32])
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
                signing_key_id: format!("constitution-service-{label}-key-{suffix}"),
                node_id: format!("constitution-service-{label}-node-{suffix}"),
                tenant_id: format!("constitution-service-{label}-tenant-{suffix}"),
                repository_id: format!("constitution-service-{label}-repository-{suffix}"),
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

    fn default_clause() -> v1::ConstitutionClause {
        v1::ConstitutionClause {
            id: "clause-a".to_string(),
            slug: "clause-a-slug".to_string(),
            kind: "constraint".to_string(),
            title: "a clause".to_string(),
            statement: "a statement".to_string(),
            status: "active".to_string(),
            consequence: "block".to_string(),
            scope: String::new(),
            rationale: "because".to_string(),
        }
    }

    fn authenticated_publish_request(
        identity: &TestIdentity,
        version_id: &str,
        nonce_byte: u8,
        schema_version: &str,
        clauses: Vec<v1::ConstitutionClause>,
    ) -> v1::PublishConstitutionSnapshotRequest {
        let key = signing_key();
        let mut authentication = v1::ConstitutionAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            nonce: vec![nonce_byte; 16],
            signature: Vec::new(),
        };
        let operation = ConstitutionOperation::Publish {
            version_id,
            version: 1,
            status: "active",
            clause_count: clauses.len() as u32,
        };
        let bytes = constitution_signature::constitution_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        v1::PublishConstitutionSnapshotRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            version_id: version_id.to_owned(),
            version: 1,
            status: "active".to_owned(),
            clauses,
            authentication: Some(authentication),
            schema_version: schema_version.to_owned(),
            source_ref: String::new(),
            content_digest: Vec::new(),
        }
    }

    #[tokio::test]
    async fn an_unauthenticated_publish_is_refused() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let store = ConstitutionStore::connect(&database_url).await.unwrap();
        let service = ConstitutionGrpcService::new(store);
        let identity = TestIdentity::fresh("unauthenticated");

        let result = service
            .publish_constitution_snapshot(Request::new(v1::PublishConstitutionSnapshotRequest {
                tenant_id: identity.tenant_id,
                repository_id: identity.repository_id,
                version_id: "version-1".to_string(),
                version: 1,
                status: "active".to_string(),
                clauses: vec![],
                authentication: None,
                schema_version: "v1".to_string(),
                source_ref: String::new(),
                content_digest: Vec::new(),
            }))
            .await;

        assert_eq!(result.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    /// Proves `authenticate` actually wires nonce consumption into the RPC
    /// path: the identical wire request granted the first time is refused
    /// the second time on the same (signing_key_id, nonce) pair.
    #[tokio::test]
    async fn an_identical_publish_is_granted_once_then_refused_as_replayed() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("replay");
        register_test_key(&database_url, &identity).await;
        let request =
            authenticated_publish_request(&identity, "version-1", 1, "v1", vec![default_clause()]);
        let store = ConstitutionStore::connect(&database_url).await.unwrap();
        let service = ConstitutionGrpcService::new(store);

        let first = service
            .publish_constitution_snapshot(Request::new(request.clone()))
            .await;
        assert!(first.is_ok());

        let second = service
            .publish_constitution_snapshot(Request::new(request))
            .await;
        assert_eq!(second.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn a_published_snapshot_is_returned_by_get_active() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("get-active");
        register_test_key(&database_url, &identity).await;
        let publish_request =
            authenticated_publish_request(&identity, "version-7", 3, "v1", vec![default_clause()]);
        let store = ConstitutionStore::connect(&database_url).await.unwrap();
        let service = ConstitutionGrpcService::new(store);

        service
            .publish_constitution_snapshot(Request::new(publish_request))
            .await
            .expect("publish should succeed");

        let key = signing_key();
        let mut authentication = v1::ConstitutionAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            nonce: vec![9; 16],
            signature: Vec::new(),
        };
        let bytes = constitution_signature::constitution_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            &ConstitutionOperation::GetActive,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();

        let response = service
            .get_active_constitution(Request::new(v1::GetActiveConstitutionRequest {
                tenant_id: identity.tenant_id,
                repository_id: identity.repository_id,
                authentication: Some(authentication),
            }))
            .await
            .expect("get_active should succeed")
            .into_inner();

        assert!(response.found);
        assert_eq!(response.version_id, "version-7");
        assert_eq!(response.clauses.len(), 1);
        assert_eq!(response.clauses[0].id, "clause-a");
    }

    #[tokio::test]
    async fn a_repository_with_no_snapshot_reports_not_found() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("not-found");
        register_test_key(&database_url, &identity).await;
        let store = ConstitutionStore::connect(&database_url).await.unwrap();
        let service = ConstitutionGrpcService::new(store);

        let key = signing_key();
        let mut authentication = v1::ConstitutionAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            nonce: vec![11; 16],
            signature: Vec::new(),
        };
        let bytes = constitution_signature::constitution_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            &ConstitutionOperation::GetActive,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();

        let response = service
            .get_active_constitution(Request::new(v1::GetActiveConstitutionRequest {
                tenant_id: identity.tenant_id,
                repository_id: identity.repository_id,
                authentication: Some(authentication),
            }))
            .await
            .expect("get_active should succeed even with nothing published")
            .into_inner();

        assert!(!response.found);
        assert!(response.clauses.is_empty());
    }

    /// ADR-0121 decision 2: the authenticated publish RPC must actually wire
    /// into the immutable history store, not just the mutable snapshot.
    #[tokio::test]
    async fn publishing_a_snapshot_also_records_immutable_publication_history() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("history");
        register_test_key(&database_url, &identity).await;
        let mut request = authenticated_publish_request(
            &identity,
            "version-history-1",
            21,
            "v1",
            vec![default_clause()],
        );
        request.source_ref = "commit:abc123".to_string();
        request.content_digest = vec![1, 2, 3, 4];
        // Re-sign: authenticated_publish_request already signed over the
        // operation-shaped fields only (version_id/version/status/clause
        // count), none of which changed, so the existing signature is still
        // valid for this mutated request.
        let store = ConstitutionStore::connect(&database_url).await.unwrap();
        let service = ConstitutionGrpcService::new(store);

        service
            .publish_constitution_snapshot(Request::new(request))
            .await
            .expect("publish should succeed");

        let verification_store = ConstitutionStore::connect(&database_url).await.unwrap();
        let recorded = verification_store
            .get_publication(
                &identity.tenant_id,
                &identity.repository_id,
                "version-history-1",
            )
            .await
            .expect("reading the recorded publication should succeed")
            .expect("the publication should have been recorded");

        assert_eq!(recorded.schema_version, "v1");
        assert_eq!(recorded.status, "active");
        assert_eq!(recorded.clauses, vec![from_proto_clause(default_clause())]);
        assert_eq!(recorded.source_reference, Some("commit:abc123".to_string()));
        assert_eq!(recorded.source_digest, Some(vec![1, 2, 3, 4]));
    }

    /// ADR-0121 decision 1: an immutability violation must refuse before the
    /// mutable active snapshot changes -- a rejected republish never silently
    /// moves the active pointer to the new, refused content.
    #[tokio::test]
    async fn republishing_the_same_version_with_different_clauses_is_refused_and_the_active_snapshot_is_unchanged(
    ) {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("immutability");
        register_test_key(&database_url, &identity).await;
        let store = ConstitutionStore::connect(&database_url).await.unwrap();
        let service = ConstitutionGrpcService::new(store);

        let first_request = authenticated_publish_request(
            &identity,
            "version-immutable-1",
            31,
            "v1",
            vec![default_clause()],
        );
        service
            .publish_constitution_snapshot(Request::new(first_request))
            .await
            .expect("the first publish should succeed");

        let mut different_clause = default_clause();
        different_clause.statement = "a completely different statement".to_string();
        let second_request = authenticated_publish_request(
            &identity,
            "version-immutable-1",
            32,
            "v1",
            vec![different_clause],
        );
        let second = service
            .publish_constitution_snapshot(Request::new(second_request))
            .await;
        assert_eq!(second.unwrap_err().code(), tonic::Code::FailedPrecondition);

        let key = signing_key();
        let mut authentication = v1::ConstitutionAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            nonce: vec![33; 16],
            signature: Vec::new(),
        };
        let bytes = constitution_signature::constitution_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            &ConstitutionOperation::GetActive,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        let active = service
            .get_active_constitution(Request::new(v1::GetActiveConstitutionRequest {
                tenant_id: identity.tenant_id,
                repository_id: identity.repository_id,
                authentication: Some(authentication),
            }))
            .await
            .expect("get_active should succeed")
            .into_inner();

        assert_eq!(active.clauses.len(), 1);
        assert_eq!(active.clauses[0].statement, "a statement");
    }

    #[tokio::test]
    async fn an_empty_schema_version_is_rejected_as_invalid_argument() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("empty-schema-version");
        register_test_key(&database_url, &identity).await;
        let request = authenticated_publish_request(
            &identity,
            "version-empty-schema",
            41,
            "",
            vec![default_clause()],
        );
        let store = ConstitutionStore::connect(&database_url).await.unwrap();
        let service = ConstitutionGrpcService::new(store);

        let result = service
            .publish_constitution_snapshot(Request::new(request))
            .await;

        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
    }
}

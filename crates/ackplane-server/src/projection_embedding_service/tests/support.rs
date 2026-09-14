use ackplane_node::SigningBinding;
use ackplane_protocol::projection_embedding_auth::{
    projection_embedding_signing_bytes, ProjectionEmbeddingOperation,
};
use ackplane_protocol::v1::{
    node_enrollment_service_server::NodeEnrollmentServiceServer,
    node_sync_service_server::NodeSyncServiceServer,
    projection_embedding_service_client::ProjectionEmbeddingServiceClient, FlowControl,
    ListMissingProjectionEmbeddingsRequest, ProjectionEmbeddingAuthentication,
    ProjectionEmbeddingSource, PublishProjectionEmbeddingRequest,
};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::{Channel, Server};

use crate::{
    db_pool::PgPool,
    enrollment_service::NodeEnrollmentService,
    enrollment_store::EnrollmentStore,
    ledger::{AppendOutcome, DedupKey, LedgerStore},
    projection::{tests::structural_fact_envelope, Projector, StructuralFact},
    service::NodeSyncService,
    test_support::{enroll_and_activate_in, test_pool, unique_id, uuid_ish},
};

use super::super::ProjectionEmbeddingService;

pub(super) struct Fixture {
    pub(super) pool: PgPool,
    pub(super) server: TestServer,
    pub(super) projector: Projector,
    pub(super) binding: SigningBinding,
}

impl Fixture {
    pub(super) async fn new() -> Self {
        Self::new_in(
            unique_id("embedding-tenant"),
            unique_id("embedding-repository"),
        )
        .await
    }

    pub(super) async fn new_in(tenant_id: String, repository_id: String) -> Self {
        let pool = test_pool().expect("projection embedding RPC tests require PostgreSQL");
        let server = TestServer::start(&pool).await;
        let projector = Projector::connect(&pool).await.unwrap();
        let seed = unique_id("embedding-node");
        let binding = SigningBinding {
            tenant_id,
            repository_id,
            node_id: format!("fleet-node-{seed}"),
            key_id: format!("fleet-signing-key-{seed}"),
        };
        enroll_and_activate_in(
            &std::env::var("ACKPLANE_TEST_DATABASE_URL").unwrap(),
            &binding.tenant_id,
            &binding.repository_id,
            &seed,
        )
        .await;
        Self {
            pool,
            server,
            projector,
            binding,
        }
    }

    pub(super) fn authentication(
        &self,
        operation: ProjectionEmbeddingOperation<'_>,
    ) -> ProjectionEmbeddingAuthentication {
        let mut authentication = ProjectionEmbeddingAuthentication {
            signing_key_id: self.binding.key_id.clone(),
            node_id: self.binding.node_id.clone(),
            signed_at: OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
            nonce: uuid_ish().to_be_bytes().to_vec(),
            signature: vec![],
        };
        authentication.signature = SigningKey::from_bytes(&[11; 32])
            .sign(&projection_embedding_signing_bytes(
                &self.binding.tenant_id,
                &self.binding.repository_id,
                &operation,
                &authentication,
            ))
            .to_bytes()
            .to_vec();
        authentication
    }

    pub(super) fn list(&self, model: &str, limit: u32) -> ListMissingProjectionEmbeddingsRequest {
        ListMissingProjectionEmbeddingsRequest {
            tenant_id: self.binding.tenant_id.clone(),
            repository_id: self.binding.repository_id.clone(),
            model: model.into(),
            limit,
            authentication: Some(
                self.authentication(ProjectionEmbeddingOperation::ListMissing { model, limit }),
            ),
        }
    }

    pub(super) fn publish(
        &self,
        source: &ProjectionEmbeddingSource,
        model: &str,
        embedding: &[f32],
    ) -> PublishProjectionEmbeddingRequest {
        PublishProjectionEmbeddingRequest {
            tenant_id: self.binding.tenant_id.clone(),
            repository_id: self.binding.repository_id.clone(),
            source: Some(source.clone()),
            model: model.into(),
            embedding: embedding.to_vec(),
            authentication: Some(self.authentication(ProjectionEmbeddingOperation::Publish {
                source,
                model,
                embedding,
            })),
        }
    }

    pub(super) async fn project(&self, sources: &[ProjectionEmbeddingSource]) {
        self.project_in(
            &self.binding.tenant_id,
            &self.binding.repository_id,
            sources,
        )
        .await;
    }

    pub(super) async fn project_in(
        &self,
        tenant_id: &str,
        repository_id: &str,
        sources: &[ProjectionEmbeddingSource],
    ) {
        let facts: Vec<_> = sources
            .iter()
            .map(|source| StructuralFact {
                node_id: source.node_id.clone(),
                node_type: "artifact".into(),
                label: source.label.clone(),
                edges: vec![],
            })
            .collect();
        self.append_facts_in(tenant_id, repository_id, &facts).await;
        self.projector
            .rebuild(tenant_id, repository_id)
            .await
            .unwrap();
    }

    pub(super) async fn append_facts_in(
        &self,
        tenant_id: &str,
        repository_id: &str,
        facts: &[StructuralFact],
    ) {
        let ledger = LedgerStore::connect(&self.pool).await.unwrap();
        let producer_id = unique_id("embedding-facts");
        for (index, fact) in facts.iter().enumerate() {
            let digest = Sha256::digest(serde_json::to_vec(fact).unwrap());
            let outcome = ledger
                .append(&structural_fact_envelope(
                    DedupKey {
                        tenant_id: tenant_id.into(),
                        repository_id: repository_id.into(),
                        producer_id: producer_id.clone(),
                        producer_sequence: i64::try_from(index + 1).unwrap(),
                    },
                    &digest,
                    fact,
                ))
                .await
                .unwrap();
            assert!(matches!(outcome, AppendOutcome::Accepted { .. }));
        }
    }

    pub(super) async fn embeddings(&self) -> Vec<(String, String, Vec<f32>)> {
        self.pool
            .get()
            .await
            .unwrap()
            .query(
                "SELECT node_id, model, embedding FROM projected_node_embeddings \
                 WHERE tenant_id = $1 AND repository_id = $2 ORDER BY node_id, model",
                &[&self.binding.tenant_id, &self.binding.repository_id],
            )
            .await
            .unwrap()
            .into_iter()
            .map(|row| {
                let vector: pgvector::Vector = row.get(2);
                (row.get(0), row.get(1), vector.as_slice().to_vec())
            })
            .collect()
    }
}

pub(super) struct TestServer {
    pub(super) endpoint: String,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), tonic::transport::Error>>>,
}

impl TestServer {
    pub(super) async fn start(pool: &PgPool) -> Self {
        let ledger = LedgerStore::connect(pool).await.unwrap();
        let projector = Projector::connect(pool).await.unwrap();
        let enrollment = EnrollmentStore::connect(pool).await.unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (shutdown, stopped) = oneshot::channel();
        let task = tokio::spawn(
            Server::builder()
                .add_service(ProjectionEmbeddingService::new(projector).into_server())
                .add_service(NodeEnrollmentServiceServer::new(
                    NodeEnrollmentService::new(enrollment),
                ))
                .add_service(NodeSyncServiceServer::new(NodeSyncService::new(
                    ledger,
                    FlowControl {
                        max_in_flight_batches: 2,
                        max_batch_bytes: 64 * 1024,
                    },
                )))
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = stopped.await;
                }),
        );
        Self {
            endpoint,
            shutdown: Some(shutdown),
            task: Some(task),
        }
    }

    pub(super) async fn client(&self) -> ProjectionEmbeddingServiceClient<Channel> {
        ProjectionEmbeddingServiceClient::connect(self.endpoint.clone())
            .await
            .unwrap()
    }

    pub(super) async fn stop(mut self) {
        self.shutdown.take().unwrap().send(()).unwrap();
        self.task.take().unwrap().await.unwrap().unwrap();
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

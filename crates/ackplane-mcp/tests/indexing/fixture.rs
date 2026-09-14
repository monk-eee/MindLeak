use std::{collections::BTreeMap, time::SystemTime};

use ackplane_protocol::v1::{
    node_enrollment_service_server::NodeEnrollmentServiceServer,
    node_sync_service_server::NodeSyncServiceServer, FlowControl,
};
use ackplane_server::{
    db_pool::{build_pool, PgPool, TEST_POOL_MAX_SIZE},
    enrollment_service::NodeEnrollmentService,
    enrollment_store::EnrollmentStore,
    ledger::{AppendOutcome, DedupKey, EventEnvelope, LedgerStore, ProvenanceClass},
    projection::{Projector, StructuralFact, STRUCTURAL_FACT_PAYLOAD_TYPE},
    projection_embedding_service::ProjectionEmbeddingService,
    service::NodeSyncService,
};
use tokio::{net::TcpListener, sync::oneshot};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

use super::{companion::TestCompanion, enrollment, service::RunningServer};

pub struct Fixture {
    pool: PgPool,
    pub tenant_id: String,
    pub repository_id: String,
    pub endpoint: String,
    pub labels: BTreeMap<String, String>,
    companion: Option<TestCompanion>,
    pub directory: tempfile::TempDir,
    server: RunningServer,
}

impl Fixture {
    pub async fn new(source_count: usize) -> Option<Self> {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return None;
        };
        let pool = build_pool(&database_url, TEST_POOL_MAX_SIZE).expect("valid test database URL");
        let enrollment_store = EnrollmentStore::connect(&pool).await.unwrap();
        let ledger = LedgerStore::connect(&pool).await.unwrap();
        let projector = Projector::connect(&pool).await.unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (shutdown, stopped) = oneshot::channel();
        let projection_service = ProjectionEmbeddingService::new(projector.clone()).into_server();
        let task = tokio::spawn(async move {
            Server::builder()
                .add_service(projection_service)
                .add_service(NodeEnrollmentServiceServer::new(
                    NodeEnrollmentService::new(enrollment_store),
                ))
                .add_service(NodeSyncServiceServer::new(NodeSyncService::new(
                    ledger,
                    FlowControl {
                        max_in_flight_batches: 16,
                        max_batch_bytes: 1_048_576,
                    },
                )))
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = stopped.await;
                })
                .await
                .expect("the fixture gRPC server must run");
        });
        let server = RunningServer::new(shutdown, task);
        let unique = format!(
            "indexing-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let tenant_id = format!("{unique}-tenant");
        let repository_id = format!("{unique}-repository");
        let binding = enrollment::activate(&pool, &endpoint, &tenant_id, &repository_id).await;
        let directory = tempfile::tempdir().unwrap();
        let companion =
            TestCompanion::start(&endpoint, binding, &enrollment::SEED, directory.path()).await;
        let labels: BTreeMap<_, _> = (0..source_count)
            .map(|index| {
                (
                    format!("artifact:index-test-{index:02}"),
                    format!("projected label {index:02}"),
                )
            })
            .collect();
        let ledger = LedgerStore::connect(&pool).await.unwrap();
        for (index, (node_id, label)) in labels.iter().enumerate() {
            let fact = StructuralFact {
                node_id: node_id.clone(),
                node_type: "artifact".into(),
                label: label.clone(),
                edges: vec![],
            };
            let outcome = ledger
                .append(&EventEnvelope {
                    key: DedupKey {
                        tenant_id: tenant_id.clone(),
                        repository_id: repository_id.clone(),
                        producer_id: "index-test-facts".into(),
                        producer_sequence: i64::try_from(index + 1).unwrap(),
                    },
                    payload: serde_json::to_vec(&fact).unwrap(),
                    payload_digest: vec![42; 32],
                    schema_version: "v1".into(),
                    occurred_at: SystemTime::now(),
                    payload_type: STRUCTURAL_FACT_PAYLOAD_TYPE.into(),
                    previous_envelope_digest: None,
                    signing_key_id: None,
                    signature: None,
                    provenance: ProvenanceClass::EnrolledNode,
                })
                .await
                .unwrap();
            assert!(matches!(outcome, AppendOutcome::Accepted { .. }));
        }
        let summary = projector.rebuild(&tenant_id, &repository_id).await.unwrap();
        assert_eq!(summary.nodes, i64::try_from(source_count).unwrap());
        Some(Self {
            pool,
            tenant_id,
            repository_id,
            endpoint,
            labels,
            companion: Some(companion),
            directory,
            server,
        })
    }

    pub async fn embeddings(&self) -> Vec<(String, String, Vec<f32>)> {
        self.pool
            .get()
            .await
            .unwrap()
            .query(
                "SELECT node_id, model, embedding::text FROM projected_node_embeddings \
             WHERE tenant_id = $1 AND repository_id = $2 ORDER BY node_id, model",
                &[&self.tenant_id, &self.repository_id],
            )
            .await
            .unwrap()
            .into_iter()
            .map(|row| {
                let encoded: String = row.get(2);
                (
                    row.get(0),
                    row.get(1),
                    serde_json::from_str(&encoded).unwrap(),
                )
            })
            .collect()
    }

    pub async fn stop(mut self) {
        drop(self.companion.take());
        self.server.stop().await;
    }
}

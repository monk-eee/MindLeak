use std::{fs, future::Future, path::PathBuf, sync::Arc, time::Duration};

use ackplane_bridge::{
    work_api::{work_routes, WorkApiState},
    work_command_api::{work_command_routes, WorkCommandApiState},
};
use ackplane_protocol::v1;
use ackplane_server::{
    claim_service::ClaimDelegationService,
    claim_store::ClaimStore,
    context_service::ContextService,
    db_pool::{build_pool, PgPool, TEST_POOL_MAX_SIZE},
    directive_store::DirectiveStore,
    fleet::FleetStore,
    ledger::LedgerStore,
    service::NodeSyncService,
    supervisor_store::SupervisorStore,
    work_command_store::WorkCommandService,
    work_query_service::WorkQueryService,
    work_store::WorkStore,
};
use axum::{
    body::Body,
    http::{header::CONTENT_TYPE, Method, Request, StatusCode},
    response::Response,
    Router,
};
use serde_json::{json, Value};
use tokio::sync::{oneshot, watch};
use tokio_stream::{
    wrappers::{ReceiverStream, TcpListenerStream},
    StreamExt,
};
use tower::ServiceExt;

use crate::supervisor_api_support::{body_json, unique_id};

pub(super) fn test_database_url() -> Option<String> {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("not run: ACKPLANE_TEST_DATABASE_URL must point to isolated ackplane_test");
        return None;
    };
    let config = database_url
        .parse::<tokio_postgres::Config>()
        .unwrap_or_else(|_| panic!("the test database configuration must be valid"));
    assert_eq!(
        config.get_dbname(),
        Some("ackplane_test"),
        "browser integration tests must never use the live database"
    );
    Some(database_url)
}

pub(super) async fn application(database_url: &str, tenant_id: &str) -> Router {
    let pool = build_pool(database_url, TEST_POOL_MAX_SIZE).expect("build isolated test pool");
    let work = Arc::new(WorkStore::connect(&pool).await.expect("connect Work store"));
    let fleet = Arc::new(
        FleetStore::connect(&pool)
            .await
            .expect("connect Fleet store"),
    );
    let commands = Arc::new(
        WorkCommandService::connect(&pool)
            .await
            .expect("connect Work command service"),
    );
    work_routes(WorkApiState::new(work, fleet.clone(), Arc::from(tenant_id))).merge(
        work_command_routes(WorkCommandApiState::new(
            commands,
            fleet,
            Arc::from(tenant_id),
        )),
    )
}

pub(super) async fn request(
    router: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> Response {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(CONTENT_TYPE, "application/json")
                .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                .expect("build browser request"),
        )
        .await
        .expect("serve browser request")
}

pub(super) async fn post_json(router: &Router, uri: &str, body: Value) -> Value {
    let response = request(router, Method::POST, uri, Some(body.clone())).await;
    assert_eq!(response.status(), StatusCode::OK, "POST {uri}: {body}");
    body_json(response).await
}

pub(super) async fn get_json(router: &Router, uri: &str) -> Value {
    let response = request(router, Method::GET, uri, None).await;
    assert_eq!(response.status(), StatusCode::OK, "GET {uri}");
    body_json(response).await
}

pub(super) fn envelope(mut payload: Value, existing_task: Option<(&str, i64)>) -> Value {
    payload["idempotency_key"] = json!(unique_id("browser-command"));
    payload["rationale"] = json!("Exercise the browser command contract");
    payload["expires_at_seconds"] = json!(4_000_000_000_u64);
    if let Some((task_id, version)) = existing_task {
        payload["existing_task_id"] = json!(task_id);
        payload["expected_task_version"] = json!(version);
    }
    payload
}

pub(super) struct TestDirectory(pub(super) PathBuf);

impl TestDirectory {
    pub(super) fn new() -> Self {
        let path = std::env::temp_dir().join(unique_id("bridge-work-runtime"));
        fs::create_dir(&path).expect("create test-owned directory");
        Self(path.canonicalize().expect("canonical test directory"))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!(
                "test directory cleanup failed for {}: {error}",
                self.0.display()
            );
        }
    }
}

pub(super) struct StopOnDrop(pub(super) watch::Sender<bool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

pub(super) async fn after_server_event<Value, Read, Reading>(
    changes: &mut watch::Receiver<()>,
    label: &str,
    mut read: Read,
) -> Value
where
    Read: FnMut() -> Reading,
    Reading: Future<Output = Option<Value>>,
{
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            changes.borrow_and_update();
            if let Some(value) = read().await {
                return value;
            }
            changes
                .changed()
                .await
                .expect("the real sync service remains available");
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {label}"))
}

struct ObservedSync {
    service: NodeSyncService,
    changes: watch::Sender<()>,
}

#[tonic::async_trait]
impl v1::node_sync_service_server::NodeSyncService for ObservedSync {
    type SynchronizeStream = ReceiverStream<Result<v1::AckplaneFrame, tonic::Status>>;

    async fn synchronize(
        &self,
        request: tonic::Request<tonic::Streaming<v1::NodeFrame>>,
    ) -> Result<tonic::Response<Self::SynchronizeStream>, tonic::Status> {
        let mut stream = self.service.synchronize(request).await?.into_inner();
        let (sender, receiver) = tokio::sync::mpsc::channel(16);
        let changes = self.changes.clone();
        tokio::spawn(async move {
            loop {
                let frame = tokio::select! {
                    _ = sender.closed() => break,
                    frame = stream.next() => frame,
                };
                let Some(frame) = frame else { break };
                changes.send_replace(());
                if sender.send(frame).await.is_err() {
                    break;
                }
            }
        });
        Ok(tonic::Response::new(ReceiverStream::new(receiver)))
    }
}

pub(super) struct SyncServer {
    pub(super) endpoint: String,
    pub(super) changes: watch::Receiver<()>,
    shutdown: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
}

impl SyncServer {
    pub(super) async fn start(pool: &PgPool) -> Self {
        let service = NodeSyncService::with_supervisor_directive_and_work_store(
            LedgerStore::connect(pool).await.expect("connect ledger"),
            SupervisorStore::connect(pool)
                .await
                .expect("connect supervisors"),
            DirectiveStore::connect(pool)
                .await
                .expect("connect directives"),
            WorkStore::connect(pool).await.expect("connect Work store"),
            v1::FlowControl {
                max_in_flight_batches: 16,
                max_batch_bytes: 1_048_576,
            },
        )
        .with_context_service(
            ContextService::connect(pool)
                .await
                .expect("connect context"),
        )
        .with_work_command_service(
            WorkCommandService::connect(pool)
                .await
                .expect("connect Work commands"),
        );
        let claims =
            ClaimDelegationService::new(ClaimStore::connect(pool).await.expect("connect claims"));
        let work =
            WorkQueryService::new(WorkStore::connect(pool).await.expect("connect Work query"));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind isolated sync service to loopback");
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let (shutdown, stopped) = oneshot::channel();
        let (changes, receiver) = watch::channel(());
        let task = tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(v1::node_sync_service_server::NodeSyncServiceServer::new(
                    ObservedSync { service, changes },
                ))
                .add_service(
                    v1::claim_delegation_service_server::ClaimDelegationServiceServer::new(claims),
                )
                .add_service(v1::work_query_service_server::WorkQueryServiceServer::new(
                    work,
                ))
                .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                    let _ = stopped.await;
                })
                .await
        });
        Self {
            endpoint,
            changes: receiver,
            shutdown: Some(shutdown),
            task,
        }
    }

    pub(super) async fn shutdown(mut self) {
        let _ = self.shutdown.take().unwrap().send(());
        tokio::time::timeout(Duration::from_secs(10), &mut self.task)
            .await
            .expect("sync server shutdown is bounded")
            .expect("sync server task must not panic")
            .expect("sync server shuts down cleanly");
    }
}

impl Drop for SyncServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        self.task.abort();
    }
}

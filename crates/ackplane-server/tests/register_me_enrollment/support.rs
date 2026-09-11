use std::{
    path::Path,
    process::Output,
    sync::{Arc, Mutex},
    time::Duration,
};

use ackplane_protocol::v1::{
    self, node_enrollment_service_server::NodeEnrollmentService as EnrollmentRpc,
    node_enrollment_service_server::NodeEnrollmentServiceServer,
    node_sync_service_server::NodeSyncServiceServer, FlowControl,
};
use ackplane_server::{
    db_pool::PgPool, enrollment_service::NodeEnrollmentService, enrollment_store::EnrollmentStore,
    ledger::LedgerStore, service::NodeSyncService,
};
use tokio::{process::Command, sync::oneshot};
use tokio_stream::wrappers::TcpListenerStream;
use tonic::{
    transport::{Identity, Server, ServerTlsConfig},
    Request, Response, Status,
};

struct EnrollmentWithLostResponse {
    inner: NodeEnrollmentService,
    dropped: Option<Arc<Mutex<Option<v1::EnrollmentActivationResult>>>>,
    dropped_request: Option<Arc<Mutex<Option<v1::EnrollmentRequestStatus>>>>,
}

#[tonic::async_trait]
impl EnrollmentRpc for EnrollmentWithLostResponse {
    async fn submit_enrollment_request(
        &self,
        request: Request<v1::EnrollmentRequest>,
    ) -> Result<Response<v1::EnrollmentRequestStatus>, Status> {
        let response = self.inner.submit_enrollment_request(request).await?;
        if let Some(dropped) = &self.dropped_request {
            let mut original = dropped.lock().unwrap();
            if original.is_none() {
                *original = Some(response.get_ref().clone());
                return Err(Status::unavailable("request response lost after commit"));
            }
        }
        Ok(response)
    }

    async fn get_activation_challenge(
        &self,
        request: Request<v1::EnrollmentChallengeRequest>,
    ) -> Result<Response<v1::EnrollmentChallenge>, Status> {
        self.inner.get_activation_challenge(request).await
    }

    async fn activate_enrollment(
        &self,
        request: Request<v1::EnrollmentActivationProof>,
    ) -> Result<Response<v1::EnrollmentActivationResult>, Status> {
        let response = self.inner.activate_enrollment(request).await?;
        if let Some(dropped) = &self.dropped {
            let mut original = dropped.lock().unwrap();
            if original.is_none() {
                *original = Some(response.get_ref().clone());
                return Err(Status::unavailable("activation response lost after commit"));
            }
        }
        Ok(response)
    }

    async fn rotate_node_key(
        &self,
        request: Request<v1::KeyRotationRequest>,
    ) -> Result<Response<v1::KeyRotationResult>, Status> {
        self.inner.rotate_node_key(request).await
    }

    async fn check_enrollment_status(
        &self,
        request: Request<v1::EnrollmentStatusRequest>,
    ) -> Result<Response<v1::EnrollmentStatusResult>, Status> {
        self.inner.check_enrollment_status(request).await
    }
}

pub(super) struct TestIdentity(Option<tempfile::TempDir>);

impl TestIdentity {
    pub(super) fn new() -> Self {
        Self(Some(tempfile::tempdir().unwrap()))
    }
    pub(super) fn path(&self) -> &Path {
        self.0.as_ref().unwrap().path()
    }

    pub(super) fn command(&self, args: &[&str], ca_path: Option<&Path>) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_register-me"));
        command
            .current_dir(self.path())
            .env_remove("MINDLEAK_ACKPLANE_KEY_PATH")
            .env_remove("MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED")
            .env_remove(ackplane_client::TLS_CA_PATH_ENV)
            .kill_on_drop(true)
            .args(args)
            .arg("--state-dir")
            .arg(self.path());
        if let Some(ca_path) = ca_path {
            command.env(ackplane_client::TLS_CA_PATH_ENV, ca_path);
        }
        command
    }

    pub(super) async fn run(&self, args: &[&str], ca_path: Option<&Path>) -> Output {
        tokio::time::timeout(
            Duration::from_secs(10),
            self.command(args, ca_path).output(),
        )
        .await
        .expect("the enrollment CLI must not hang")
        .expect("the enrollment CLI must start")
    }

    pub(super) fn remove_credential(&self) -> Result<(), String> {
        let bytes = match std::fs::read(self.path().join("enrolment.json")) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err("could not read test credential metadata for cleanup".to_string()),
        };
        let record: ackplane_node::EnrolmentRecord = serde_json::from_slice(&bytes)
            .map_err(|_| "invalid test credential metadata for cleanup".to_string())?;
        let handle = record
            .provider_handle
            .as_deref()
            .ok_or("test credential handle missing")?;
        if record.provider_scheme != "credential-facility-software"
            || handle.len() != 32
            || !handle.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("refusing cleanup of an unexpected test credential".to_string());
        }
        let service = "mindleak-ackplane-node-software-v1";
        #[cfg(target_os = "macos")]
        {
            use security_framework::item::{ItemClass, ItemSearchOptions, Reference, SearchResult};

            let mut query = ItemSearchOptions::new();
            query
                .class(ItemClass::generic_password())
                .service(service)
                .account(handle)
                .load_refs(true);
            let items = match query.search() {
                Ok(items) => items,
                Err(error) if error.code() == -25300 => return Ok(()),
                Err(error) => {
                    return Err(format!(
                        "test Keychain reference lookup failed ({})",
                        error.code()
                    ))
                }
            };
            for item in items {
                match item {
                    SearchResult::Ref(Reference::KeychainItem(item)) => item.delete(),
                    _ => return Err("unexpected test Keychain reference type".to_string()),
                }
            }
            match query.search() {
                Err(error) if error.code() == -25300 => Ok(()),
                Err(error) => Err(format!(
                    "test Keychain cleanup verification failed ({})",
                    error.code()
                )),
                Ok(_) => Err("test Keychain entry remains after deletion".to_string()),
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            match keyring::Entry::new(service, handle).and_then(|entry| entry.delete_password()) {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_) => Err("could not remove the exact test credential entry".to_string()),
            }
        }
    }
}

impl Drop for TestIdentity {
    fn drop(&mut self) {
        if let Err(error) = self.remove_credential() {
            let path = self.0.take().unwrap().keep();
            let message = format!(
                "test credential cleanup failed: {error}; metadata retained at {}",
                path.display()
            );
            if std::thread::panicking() {
                eprintln!("{message}");
            } else {
                panic!("{message}");
            }
        }
    }
}

pub(super) async fn start_server(
    pool: &PgPool,
    with_sync: bool,
    dropped: Option<Arc<Mutex<Option<v1::EnrollmentActivationResult>>>>,
    dropped_request: Option<Arc<Mutex<Option<v1::EnrollmentRequestStatus>>>>,
    tls_identity: Option<Identity>,
) -> (String, oneshot::Sender<()>, tokio::task::JoinHandle<()>) {
    let enrollment = EnrollmentStore::connect(pool).await.unwrap();
    let sync = if with_sync {
        Some(NodeSyncServiceServer::new(
            NodeSyncService::with_supervisor_store(
                LedgerStore::connect(pool).await.unwrap(),
                ackplane_server::supervisor_store::SupervisorStore::connect(pool)
                    .await
                    .unwrap(),
                FlowControl {
                    max_in_flight_batches: 4,
                    max_batch_bytes: 1_048_576,
                },
            ),
        ))
    } else {
        None
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let scheme = if tls_identity.is_some() {
        "https"
    } else {
        "http"
    };
    let endpoint = format!("{scheme}://{}", listener.local_addr().unwrap());
    let mut builder = Server::builder();
    if let Some(identity) = tls_identity {
        builder = builder
            .tls_config(ServerTlsConfig::new().identity(identity))
            .unwrap();
    }
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        builder
            .add_service(NodeEnrollmentServiceServer::new(
                EnrollmentWithLostResponse {
                    inner: NodeEnrollmentService::new(enrollment),
                    dropped,
                    dropped_request,
                },
            ))
            .add_optional_service(sync)
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = shutdown_rx.await;
            })
            .await
            .unwrap();
    });
    (endpoint, shutdown_tx, server)
}

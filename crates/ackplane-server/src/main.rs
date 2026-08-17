//! The Ackplane federation service entry point (ADR-0082).

use std::{fs, process::ExitCode};

use ackplane_protocol::v1::{
    self, node_enrollment_service_server::NodeEnrollmentServiceServer,
    node_sync_service_server::NodeSyncServiceServer,
};
use ackplane_server::{
    enrollment_service::NodeEnrollmentService, enrollment_store::EnrollmentStore,
    ledger::LedgerStore, service::NodeSyncService, ServerConfig,
};

#[tokio::main]
async fn main() -> ExitCode {
    // JSON so decision 10's fields (method, outcome, reason, latency_ms,
    // batch_records, batch_bytes, retry_count, position) stay structured
    // rather than becoming another hand-parsed log line.
    tracing_subscriber::fmt().json().init();
    match ServerConfig::resolve(|key| std::env::var(key).ok()) {
        Ok(config) => {
            println!("{}", config.banner());
            let tls = match config.tls.as_ref().map(load_tls).transpose() {
                Ok(tls) => tls,
                Err(error) => {
                    eprintln!("ackplane-server: could not load TLS material: {error}");
                    return ExitCode::FAILURE;
                }
            };
            let ledger = match LedgerStore::connect(config.database_url()).await {
                Ok(ledger) => ledger,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured ledger: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let enrollment_store = match EnrollmentStore::connect(config.database_url()).await {
                Ok(store) => store,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured enrollment store: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };

            println!(
                "ackplane-server: serving NodeSyncService.Synchronize and NodeEnrollmentService"
            );
            let server = tonic::transport::Server::builder();
            let mut server = match tls {
                Some(tls) => match server.tls_config(tls) {
                    Ok(server) => server,
                    Err(error) => {
                        eprintln!("ackplane-server: could not configure TLS: {error}");
                        return ExitCode::FAILURE;
                    }
                },
                None => server,
            };
            match server
                .add_service(NodeSyncServiceServer::new(NodeSyncService::new(
                    ledger,
                    v1::FlowControl {
                        max_in_flight_batches: config.max_in_flight_batches,
                        max_batch_bytes: config.max_batch_bytes,
                    },
                )))
                .add_service(NodeEnrollmentServiceServer::new(
                    NodeEnrollmentService::new(enrollment_store),
                ))
                .serve(config.listen)
                .await
            {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("ackplane-server: gRPC server stopped with an error: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        Err(error) => {
            eprintln!("ackplane-server: {error}");
            ExitCode::FAILURE
        }
    }
}

fn load_tls(
    paths: &ackplane_server::TlsPaths,
) -> Result<tonic::transport::ServerTlsConfig, String> {
    let certificate =
        fs::read(&paths.certificate).map_err(|error| format!("{}: {error}", paths.certificate))?;
    let key = fs::read(&paths.key).map_err(|error| format!("{}: {error}", paths.key))?;
    Ok(tonic::transport::ServerTlsConfig::new()
        .identity(tonic::transport::Identity::from_pem(certificate, key)))
}

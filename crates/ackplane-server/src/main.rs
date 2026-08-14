//! The Ackplane federation service entry point (ADR-0082).

use std::{net::SocketAddr, process::ExitCode};

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
    match ServerConfig::resolve(|key| std::env::var(key).ok()) {
        Ok(config) => {
            println!("{}", config.banner());
            let address = match config.listen.parse::<SocketAddr>() {
                Ok(address) => address,
                Err(error) => {
                    eprintln!("ackplane-server: invalid ACKPLANE_LISTEN address: {error}");
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
            match tonic::transport::Server::builder()
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
                .serve(address)
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

//! The Ackplane federation service entry point (ADR-0082).

use std::{fs, process::ExitCode};

use ackplane_protocol::v1::{
    self, claim_delegation_service_server::ClaimDelegationServiceServer,
    knowledge_service_server::KnowledgeServiceServer,
    node_enrollment_service_server::NodeEnrollmentServiceServer,
    node_sync_service_server::NodeSyncServiceServer,
};
use ackplane_server::{
    claim_service::ClaimDelegationService,
    claim_store::ClaimStore,
    enrollment_service::NodeEnrollmentService,
    enrollment_store::EnrollmentStore,
    knowledge_service::KnowledgeGrpcService,
    knowledge_store::KnowledgeStore,
    ledger::LedgerStore,
    projection::{run_projection_worker, Projector},
    service::NodeSyncService,
    ServerConfig,
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
            let claim_store = match ClaimStore::connect(config.database_url()).await {
                Ok(store) => store,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured claim authority: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let projector = match Projector::connect(config.database_url()).await {
                Ok(projector) => projector,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured projection store: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let knowledge_store = match KnowledgeStore::connect(config.database_url()).await {
                Ok(store) => store,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured knowledge store: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            // ADR-0086 clause 9: a projection worker reads the durable ledger
            // through checkpoints on its own cadence, decoupled from request
            // handling; a stalled or errored tick never stops the gRPC server.
            tokio::spawn(run_projection_worker(
                projector,
                std::time::Duration::from_secs(config.projection_interval_secs as u64),
            ));

            println!(
                "ackplane-server: serving NodeSyncService.Synchronize, NodeEnrollmentService, ClaimDelegationService, and KnowledgeService"
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
                .add_service(ClaimDelegationServiceServer::new(
                    ClaimDelegationService::new(claim_store),
                ))
                .add_service(KnowledgeServiceServer::new(KnowledgeGrpcService::new(
                    knowledge_store,
                )))
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

//! The Ackplane federation service entry point (ADR-0082).

use std::{fs, process::ExitCode};

use ackplane_protocol::v1::{
    self, claim_delegation_service_server::ClaimDelegationServiceServer,
    constitution_service_server::ConstitutionServiceServer,
    evidence_service_server::EvidenceServiceServer,
    knowledge_service_server::KnowledgeServiceServer,
    node_enrollment_service_server::NodeEnrollmentServiceServer,
    node_sync_service_server::NodeSyncServiceServer,
    telemetry_service_server::TelemetryServiceServer,
    work_query_service_server::WorkQueryServiceServer,
};
use ackplane_server::{
    claim_service::ClaimDelegationService,
    claim_store::ClaimStore,
    constitution_service::ConstitutionGrpcService,
    constitution_store::ConstitutionStore,
    directive_store::DirectiveStore,
    enrollment_service::NodeEnrollmentService,
    enrollment_store::EnrollmentStore,
    evidence_service::EvidenceGrpcService,
    evidence_store::EvidenceStore,
    knowledge_service::KnowledgeGrpcService,
    knowledge_store::KnowledgeStore,
    ledger::LedgerStore,
    projection::{run_projection_worker, Projector},
    service::NodeSyncService,
    supervisor_store::SupervisorStore,
    telemetry_service::TelemetryGrpcService,
    telemetry_store::TelemetryStore,
    work_query_service::WorkQueryService,
    work_store::WorkStore,
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
            // One pool for this process (ADR-0143 decision 1). Stores still on
            // their own `connect(database_url)` take their turn in the
            // migration sequence; none is left half-migrated.
            let db_pool = match ackplane_server::db_pool::build_pool(
                config.database_url(),
                ackplane_server::db_pool::SERVICE_POOL_MAX_SIZE,
            ) {
                Ok(pool) => pool,
                Err(error) => {
                    eprintln!("ackplane-server: could not build the database pool: {error}");
                    return ExitCode::FAILURE;
                }
            };
            let ledger = match LedgerStore::connect(&db_pool).await {
                Ok(ledger) => ledger,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured ledger: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let enrollment_store = match EnrollmentStore::connect(&db_pool).await {
                Ok(store) => store,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured enrollment store: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let claim_store = match ClaimStore::connect(&db_pool).await {
                Ok(store) => store,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured claim authority: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let projector = match Projector::connect(&db_pool).await {
                Ok(projector) => projector,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured projection store: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let knowledge_store = match KnowledgeStore::connect(&db_pool).await {
                Ok(store) => store,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured knowledge store: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let evidence_store = match EvidenceStore::connect(&db_pool).await {
                Ok(store) => store,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured evidence store: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let constitution_store = match ConstitutionStore::connect(&db_pool).await {
                Ok(store) => store,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured constitution store: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let telemetry_store = match TelemetryStore::connect(&db_pool).await {
                Ok(store) => store,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured telemetry store: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let supervisor_store = match SupervisorStore::connect(&db_pool).await {
                Ok(store) => store,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured supervisor store: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let directive_store = match DirectiveStore::connect(&db_pool).await {
                Ok(store) => store,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured directive ledger: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            let work_store = match WorkStore::connect(&db_pool).await {
                Ok(store) => store,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured Industrial Work store: {error}"
                    );
                    return ExitCode::FAILURE;
                }
            };
            // A second connection, matching every other store's already-
            // existing one-connection-per-service pattern here (ADR-0143's
            // one-bounded-pool-per-process consolidation is separate,
            // in-progress work; this read service does not get ahead of it).
            let work_query_store = match WorkStore::connect(&db_pool).await {
                Ok(store) => store,
                Err(error) => {
                    eprintln!(
                        "ackplane-server: could not connect to the configured Industrial Work \
                         query store: {error}"
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
                "ackplane-server: serving NodeSyncService.Synchronize, NodeEnrollmentService, ClaimDelegationService, KnowledgeService, EvidenceService, ConstitutionService, TelemetryService, WorkQueryService, authenticated supervisor facts, directive receipts, and native Industrial Work ingress"
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
                .add_service(NodeSyncServiceServer::new(
                    NodeSyncService::with_supervisor_directive_and_work_store(
                        ledger,
                        supervisor_store,
                        directive_store,
                        work_store,
                        v1::FlowControl {
                            max_in_flight_batches: config.max_in_flight_batches,
                            max_batch_bytes: config.max_batch_bytes,
                        },
                    ),
                ))
                .add_service(NodeEnrollmentServiceServer::new(
                    NodeEnrollmentService::new(enrollment_store),
                ))
                .add_service(ClaimDelegationServiceServer::new(
                    ClaimDelegationService::new(claim_store),
                ))
                .add_service(KnowledgeServiceServer::new(KnowledgeGrpcService::new(
                    knowledge_store,
                )))
                .add_service(EvidenceServiceServer::new(EvidenceGrpcService::new(
                    evidence_store,
                )))
                .add_service(ConstitutionServiceServer::new(
                    ConstitutionGrpcService::new(constitution_store),
                ))
                .add_service(TelemetryServiceServer::new(TelemetryGrpcService::new(
                    telemetry_store,
                )))
                .add_service(WorkQueryServiceServer::new(WorkQueryService::new(
                    work_query_store,
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

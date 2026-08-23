use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use ackplane_bridge::administration::{administration_routes, AdministrationApiState};
use ackplane_bridge::context_api::{context_routes, ContextApiState};
use ackplane_bridge::evidence::BridgeEvidenceStore;
use ackplane_bridge::evidence_api::{evidence_routes, EvidenceApiState};
use ackplane_bridge::knowledge_api::{knowledge_routes, KnowledgeApiState};
use ackplane_bridge::BridgeConfig;
use ackplane_server::claim_store::ClaimStore;
use ackplane_server::constitution_store::ConstitutionStore;
use ackplane_server::context_packet_store::ContextPacketStore;
use ackplane_server::fleet::{FleetStore, RepositoryFreshness};
use ackplane_server::knowledge_store::KnowledgeStore;
use ackplane_server::projection::Projector;
use ackplane_server::readiness::ReadinessStore;
use ackplane_server::telemetry_store::TelemetryStore;
use axum::{
    response::{Html, IntoResponse},
    routing::{get, post},
    Router,
};
use tokio::sync::Mutex;

mod handlers;

use handlers::{
    agents, fleet, readiness, repository_claims, repository_constitution, repository_detail,
    repository_graph, repository_knowledge, repository_recover_claim, repository_signing_keys,
    repository_stranded_claims, repository_telemetry, repository_timeline, telemetry_page,
};

const FLEET_PAGE: &str = include_str!("../static/index.html");

#[derive(Clone)]
struct AppState {
    fleet: Arc<FleetStore>,
    knowledge: Arc<KnowledgeStore>,
    constitution: Arc<ConstitutionStore>,
    claims: Arc<Mutex<ClaimStore>>,
    projector: Arc<Projector>,
    readiness: Arc<ReadinessStore>,
    telemetry: Arc<TelemetryStore>,
    tenant_id: Arc<str>,
}

fn unix_seconds(timestamp: SystemTime) -> Option<u64> {
    timestamp
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

/// Shared with `RepositoryDetailResponse` so the Fleet list and a single
/// repository's detail can never report the same freshness under two
/// different strings.
fn freshness_label(freshness: RepositoryFreshness) -> &'static str {
    match freshness {
        RepositoryFreshness::NeverProjected => "never_projected",
        RepositoryFreshness::Lagging => "lagging",
        RepositoryFreshness::Fresh => "fresh",
    }
}

#[tokio::main]
async fn main() {
    let salt_path = match std::env::var("ACKPLANE_BRIDGE_SALT_PATH") {
        Ok(raw) if !raw.trim().is_empty() => std::path::PathBuf::from(raw.trim()),
        _ => {
            eprintln!(
                "ackplane-bridge: ACKPLANE_BRIDGE_SALT_PATH must be set for the loopback developer profile"
            );
            return;
        }
    };
    let salt = match ackplane_bridge::load_or_generate_salt(&salt_path) {
        Ok(salt) => salt,
        Err(error) => {
            eprintln!(
                "ackplane-bridge: could not load or generate the developer-tenant salt: {error}"
            );
            return;
        }
    };
    let config = match BridgeConfig::resolve(|key| std::env::var(key).ok(), &salt) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("ackplane-bridge: {error}");
            return;
        }
    };
    let fleet_store = match FleetStore::connect(config.database_url()).await {
        Ok(fleet) => Arc::new(fleet),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane read models: {error}");
            return;
        }
    };
    let knowledge_store = match KnowledgeStore::connect(config.database_url()).await {
        Ok(knowledge) => Arc::new(knowledge),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane knowledge domain: {error}");
            return;
        }
    };
    let constitution_store = match ConstitutionStore::connect(config.database_url()).await {
        Ok(constitution) => Arc::new(constitution),
        Err(error) => {
            eprintln!(
                "ackplane-bridge: could not connect to Ackplane constitution domain: {error}"
            );
            return;
        }
    };
    let claim_store = match ClaimStore::connect(config.database_url()).await {
        Ok(claims) => Arc::new(Mutex::new(claims)),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane claim delegation: {error}");
            return;
        }
    };
    let projector = match Projector::connect(config.database_url()).await {
        Ok(projector) => Arc::new(projector),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane's graph projection: {error}");
            return;
        }
    };
    let readiness_store = match ReadinessStore::connect(config.database_url()).await {
        Ok(readiness) => Arc::new(readiness),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane readiness rollup: {error}");
            return;
        }
    };
    let telemetry_store = match TelemetryStore::connect(config.database_url()).await {
        Ok(telemetry) => Arc::new(telemetry),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane telemetry domain: {error}");
            return;
        }
    };
    let evidence_store = match BridgeEvidenceStore::connect(config.database_url()).await {
        Ok(evidence) => Arc::new(evidence),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane evidence domain: {error}");
            return;
        }
    };
    let context_packet_store = match ContextPacketStore::connect(config.database_url()).await {
        Ok(context_packets) => Arc::new(context_packets),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane context packets: {error}");
            return;
        }
    };
    let tenant_id: Arc<str> = Arc::from(config.development_tenant_token.clone());
    let evidence_api_state =
        EvidenceApiState::new(evidence_store, fleet_store.clone(), tenant_id.clone());
    let knowledge_api_state = KnowledgeApiState::new(
        knowledge_store.clone(),
        fleet_store.clone(),
        tenant_id.clone(),
    );
    let context_api_state =
        ContextApiState::new(context_packet_store, fleet_store.clone(), tenant_id.clone());
    let administration_api_state =
        AdministrationApiState::new(fleet_store.clone(), tenant_id.clone());
    let state = AppState {
        fleet: fleet_store,
        knowledge: knowledge_store,
        constitution: constitution_store,
        claims: claim_store,
        projector,
        readiness: readiness_store,
        telemetry: telemetry_store,
        tenant_id,
    };
    let application = Router::new()
        .route("/", get(fleet_page))
        .route("/telemetry", get(telemetry_page))
        .route("/api/v1/fleet", get(fleet))
        .route("/api/v1/agents", get(agents))
        .route("/api/v1/readiness", get(readiness))
        .route(
            "/api/v1/repositories/:repository_id",
            get(repository_detail),
        )
        .route(
            "/api/v1/repositories/:repository_id/timeline",
            get(repository_timeline),
        )
        .route(
            "/api/v1/repositories/:repository_id/claims",
            get(repository_claims),
        )
        .route(
            "/api/v1/repositories/:repository_id/stranded-claims",
            get(repository_stranded_claims),
        )
        .route(
            "/api/v1/repositories/:repository_id/signing-keys",
            get(repository_signing_keys),
        )
        .route(
            "/api/v1/repositories/:repository_id/knowledge",
            get(repository_knowledge),
        )
        .route(
            "/api/v1/repositories/:repository_id/graph",
            get(repository_graph),
        )
        .route(
            "/api/v1/repositories/:repository_id/constitution",
            get(repository_constitution),
        )
        .route(
            "/api/v1/repositories/:repository_id/telemetry",
            get(repository_telemetry),
        )
        .route(
            "/api/v1/repositories/:repository_id/tasks/:task_id/recover",
            post(repository_recover_claim),
        )
        .with_state(state)
        .merge(evidence_routes(evidence_api_state))
        .merge(knowledge_routes(knowledge_api_state))
        .merge(context_routes(context_api_state))
        .merge(administration_routes(administration_api_state));
    let listener = match tokio::net::TcpListener::bind(config.listen).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!(
                "ackplane-bridge: could not listen on {}: {error}",
                config.listen
            );
            return;
        }
    };
    println!(
        "ackplane-bridge: serving Fleet for development tenant on http://{}",
        config.listen
    );
    if let Err(error) = axum::serve(listener, application).await {
        eprintln!("ackplane-bridge: server stopped with an error: {error}");
    }
}

async fn fleet_page() -> impl IntoResponse {
    Html(FLEET_PAGE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fleet_page_refreshes_fleet_agents_and_readiness_only_while_visible() {
        let response = fleet_page().await.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("reading the fleet page body");
        let body = String::from_utf8(body.to_vec()).expect("fleet page body is valid UTF-8");

        for required in [
            "function startVisibleRefresh(load, intervalMs)",
            "document.visibilityState === \"visible\"",
            "document.addEventListener(\"visibilitychange\"",
            "startVisibleRefresh(loadFleet, REFRESH_INTERVAL_MS)",
            "startVisibleRefresh(loadAgents, REFRESH_INTERVAL_MS)",
            "startVisibleRefresh(loadReadiness, REFRESH_INTERVAL_MS)",
        ] {
            assert!(body.contains(required), "index.html is missing {required}");
        }
    }
}

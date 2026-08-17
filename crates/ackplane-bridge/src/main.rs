use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use ackplane_bridge::BridgeConfig;
use ackplane_server::fleet::{FleetRepository, FleetStore};
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use serde::Serialize;

const FLEET_PAGE: &str = include_str!("../static/index.html");

#[derive(Clone)]
struct AppState {
    fleet: Arc<FleetStore>,
    tenant_id: Arc<str>,
}

#[derive(Serialize)]
struct FleetResponse {
    repositories: Vec<FleetSummary>,
}

#[derive(Serialize)]
struct FleetSummary {
    repository_id: String,
    active_node_count: i64,
    last_activated_at_seconds: Option<u64>,
    projection_stream_position: Option<i64>,
    projection_updated_at_seconds: Option<u64>,
}

impl From<FleetRepository> for FleetSummary {
    fn from(repository: FleetRepository) -> Self {
        Self {
            repository_id: repository.repository_id,
            active_node_count: repository.active_node_count,
            last_activated_at_seconds: unix_seconds(repository.last_activated_at),
            projection_stream_position: repository.projection_stream_position,
            projection_updated_at_seconds: repository.projection_updated_at.and_then(unix_seconds),
        }
    }
}

fn unix_seconds(timestamp: SystemTime) -> Option<u64> {
    timestamp
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

#[tokio::main]
async fn main() {
    let config = match BridgeConfig::resolve(|key| std::env::var(key).ok()) {
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
    let state = AppState {
        fleet: fleet_store,
        tenant_id: Arc::from(config.development_tenant),
    };
    let application = Router::new()
        .route("/", get(fleet_page))
        .route("/api/v1/fleet", get(fleet))
        .with_state(state);
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

async fn fleet(State(state): State<AppState>) -> Result<Json<FleetResponse>, StatusCode> {
    state
        .fleet
        .repositories(&state.tenant_id)
        .await
        .map(|repositories| {
            Json(FleetResponse {
                repositories: repositories.into_iter().map(FleetSummary::from).collect(),
            })
        })
        .map_err(|error| {
            tracing::error!(%error, "Bridge Fleet query failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn fleet_page() -> impl IntoResponse {
    Html(FLEET_PAGE)
}

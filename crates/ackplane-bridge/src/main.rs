use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use ackplane_bridge::BridgeConfig;
use ackplane_server::fleet::{
    FleetRepository, FleetStore, RepositoryDetail, RepositoryFreshness, TimelineEvent,
};
use axum::{
    extract::{Path, State},
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

#[derive(Serialize)]
struct RepositoryDetailResponse {
    repository_id: String,
    active_node_count: i64,
    last_activated_at_seconds: Option<u64>,
    ledger_stream_position: i64,
    projection_stream_position: Option<i64>,
    projection_updated_at_seconds: Option<u64>,
    freshness: &'static str,
}

impl From<RepositoryDetail> for RepositoryDetailResponse {
    fn from(detail: RepositoryDetail) -> Self {
        let freshness = match detail.freshness {
            RepositoryFreshness::NeverProjected => "never_projected",
            RepositoryFreshness::Lagging => "lagging",
            RepositoryFreshness::Fresh => "fresh",
        };
        Self {
            repository_id: detail.repository_id,
            active_node_count: detail.active_node_count,
            last_activated_at_seconds: unix_seconds(detail.last_activated_at),
            ledger_stream_position: detail.ledger_stream_position,
            projection_stream_position: detail.projection_stream_position,
            projection_updated_at_seconds: detail.projection_updated_at.and_then(unix_seconds),
            freshness,
        }
    }
}

#[derive(Serialize)]
struct TimelineResponse {
    events: Vec<TimelineEventSummary>,
}

#[derive(Serialize)]
struct TimelineEventSummary {
    stream_position: i64,
    occurred_at_seconds: Option<u64>,
    payload_type: String,
    producer_id: String,
}

impl From<TimelineEvent> for TimelineEventSummary {
    fn from(event: TimelineEvent) -> Self {
        Self {
            stream_position: event.stream_position,
            occurred_at_seconds: unix_seconds(event.occurred_at),
            payload_type: event.payload_type,
            producer_id: event.producer_id,
        }
    }
}

/// How many timeline events one request returns. A first, fixed slice rather
/// than caller-controlled paging - ADR-0095 does not yet define a paging
/// contract, and an unbounded limit would let a request pull an entire
/// repository's ledger history through the Bridge.
const TIMELINE_LIMIT: i64 = 50;

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
    let state = AppState {
        fleet: fleet_store,
        tenant_id: Arc::from(config.development_tenant_token),
    };
    let application = Router::new()
        .route("/", get(fleet_page))
        .route("/api/v1/fleet", get(fleet))
        .route(
            "/api/v1/repositories/:repository_id",
            get(repository_detail),
        )
        .route(
            "/api/v1/repositories/:repository_id/timeline",
            get(repository_timeline),
        )
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

async fn repository_detail(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
) -> Result<Json<RepositoryDetailResponse>, StatusCode> {
    match state
        .fleet
        .repository(&state.tenant_id, &repository_id)
        .await
    {
        Ok(Some(detail)) => Ok(Json(detail.into())),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(error) => {
            tracing::error!(%error, "Bridge repository detail query failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn repository_timeline(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
) -> Result<Json<TimelineResponse>, StatusCode> {
    // A repository outside the caller's tenant must read exactly like one
    // that was never enrolled: check enrolment first so a non-existent
    // timeline never leaks a 200 for a repository this tenant cannot see.
    match state
        .fleet
        .repository(&state.tenant_id, &repository_id)
        .await
    {
        Ok(Some(_)) => {}
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(error) => {
            tracing::error!(%error, "Bridge repository timeline lookup failed");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    match state
        .fleet
        .timeline(&state.tenant_id, &repository_id, TIMELINE_LIMIT)
        .await
    {
        Ok(events) => Ok(Json(TimelineResponse {
            events: events.into_iter().map(TimelineEventSummary::from).collect(),
        })),
        Err(error) => {
            tracing::error!(%error, "Bridge repository timeline query failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

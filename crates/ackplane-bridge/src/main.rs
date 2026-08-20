use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use ackplane_bridge::BridgeConfig;
use ackplane_server::fleet::{
    ActiveWorkItem, FleetRepository, FleetStore, RepositoryDetail, RepositoryFreshness,
    SigningKeyStatus, TimelineEvent,
};
use ackplane_server::knowledge_store::{ActiveKnowledge, KnowledgeStore};
use ackplane_server::signing_keys::KeyResolution;
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
    knowledge: Arc<KnowledgeStore>,
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
    freshness: &'static str,
}

impl From<FleetRepository> for FleetSummary {
    fn from(repository: FleetRepository) -> Self {
        Self {
            repository_id: repository.repository_id,
            active_node_count: repository.active_node_count,
            last_activated_at_seconds: unix_seconds(repository.last_activated_at),
            projection_stream_position: repository.projection_stream_position,
            projection_updated_at_seconds: repository.projection_updated_at.and_then(unix_seconds),
            freshness: freshness_label(repository.freshness),
        }
    }
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
        Self {
            repository_id: detail.repository_id,
            active_node_count: detail.active_node_count,
            last_activated_at_seconds: unix_seconds(detail.last_activated_at),
            ledger_stream_position: detail.ledger_stream_position,
            projection_stream_position: detail.projection_stream_position,
            projection_updated_at_seconds: detail.projection_updated_at.and_then(unix_seconds),
            freshness: freshness_label(detail.freshness),
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
    signing_key_id: Option<String>,
    /// The same status word a repository's signing-keys list would report
    /// for this key at this instant (ADR-0084 decision 12), so a timeline
    /// entry can visibly flag a key that has since been revoked without
    /// altering the event's own recorded fields above. `None` when the
    /// event carries no signing key.
    key_status: Option<&'static str>,
}

impl From<TimelineEvent> for TimelineEventSummary {
    fn from(event: TimelineEvent) -> Self {
        Self {
            stream_position: event.stream_position,
            occurred_at_seconds: unix_seconds(event.occurred_at),
            payload_type: event.payload_type,
            producer_id: event.producer_id,
            signing_key_id: event.signing_key_id,
            key_status: event.key_status.as_ref().map(key_resolution_label),
        }
    }
}

#[derive(Serialize)]
struct ActiveWorkResponse {
    claims: Vec<ActiveWorkSummary>,
}

#[derive(Serialize)]
struct ActiveWorkSummary {
    task_id: String,
    owner_id: String,
    branch: String,
    lease_expires_at_seconds: Option<u64>,
    paths: Vec<String>,
    symbols: Vec<String>,
}

impl From<ActiveWorkItem> for ActiveWorkSummary {
    fn from(item: ActiveWorkItem) -> Self {
        Self {
            task_id: item.task_id,
            owner_id: item.owner_id,
            branch: item.branch,
            lease_expires_at_seconds: unix_seconds(item.lease_expires_at),
            paths: item.paths,
            symbols: item.symbols,
        }
    }
}

#[derive(Serialize)]
struct SigningKeysResponse {
    keys: Vec<SigningKeyStatusSummary>,
}

#[derive(Serialize)]
struct SigningKeyStatusSummary {
    signing_key_id: String,
    node_id: String,
    public_key_fingerprint: String,
    status: &'static str,
    expires_at_seconds: Option<u64>,
}

impl From<SigningKeyStatus> for SigningKeyStatusSummary {
    fn from(key: SigningKeyStatus) -> Self {
        Self {
            signing_key_id: key.signing_key_id,
            node_id: key.node_id,
            public_key_fingerprint: key.public_key_fingerprint,
            status: key_resolution_label(&key.status),
            expires_at_seconds: key.expires_at.and_then(unix_seconds),
        }
    }
}

/// A repository detail page's key-health row must read the same status word
/// an envelope's own verification would report for a key at this instant.
fn key_resolution_label(resolution: &KeyResolution) -> &'static str {
    match resolution {
        KeyResolution::Resolved(_) => "resolved",
        KeyResolution::Unknown => "unknown",
        KeyResolution::BindingMismatch => "binding_mismatch",
        KeyResolution::NotYetActive => "not_yet_active",
        KeyResolution::Expired => "expired",
        KeyResolution::Revoked => "revoked",
        KeyResolution::Retired => "retired",
    }
}

/// How many timeline events one request returns. A first, fixed slice rather
/// than caller-controlled paging - ADR-0095 does not yet define a paging
/// contract, and an unbounded limit would let a request pull an entire
/// repository's ledger history through the Bridge.
const TIMELINE_LIMIT: i64 = 50;
const ACTIVE_WORK_LIMIT: i64 = 50;
/// Same fixed-slice rationale as `TIMELINE_LIMIT`, applied to a knowledge recall.
const KNOWLEDGE_LIMIT: i64 = 50;

#[derive(Serialize)]
struct KnowledgeResponse {
    entries: Vec<KnowledgeEntrySummary>,
}

#[derive(Serialize)]
struct KnowledgeEntrySummary {
    knowledge_id: String,
    content: String,
    source_ref: Option<String>,
    effective_weight: f64,
    confirmed_at_seconds: Option<u64>,
}

impl From<ActiveKnowledge> for KnowledgeEntrySummary {
    fn from(entry: ActiveKnowledge) -> Self {
        Self {
            knowledge_id: entry.knowledge_id,
            content: entry.content,
            source_ref: entry.source_ref,
            effective_weight: entry.effective_weight,
            confirmed_at_seconds: unix_seconds(entry.confirmed_at),
        }
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
    let state = AppState {
        fleet: fleet_store,
        knowledge: knowledge_store,
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
        .route(
            "/api/v1/repositories/:repository_id/claims",
            get(repository_claims),
        )
        .route(
            "/api/v1/repositories/:repository_id/signing-keys",
            get(repository_signing_keys),
        )
        .route(
            "/api/v1/repositories/:repository_id/knowledge",
            get(repository_knowledge),
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

async fn repository_claims(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
) -> Result<Json<ActiveWorkResponse>, StatusCode> {
    match state
        .fleet
        .active_work(
            &state.tenant_id,
            &repository_id,
            SystemTime::now(),
            ACTIVE_WORK_LIMIT,
        )
        .await
    {
        Ok(Some(claims)) => Ok(Json(ActiveWorkResponse {
            claims: claims.into_iter().map(ActiveWorkSummary::from).collect(),
        })),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(error) => {
            tracing::error!(%error, "Bridge repository active-work query failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn repository_signing_keys(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
) -> Result<Json<SigningKeysResponse>, StatusCode> {
    match state
        .fleet
        .signing_keys(&state.tenant_id, &repository_id)
        .await
    {
        Ok(Some(keys)) => Ok(Json(SigningKeysResponse {
            keys: keys
                .into_iter()
                .map(SigningKeyStatusSummary::from)
                .collect(),
        })),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(error) => {
            tracing::error!(%error, "Bridge repository signing-key health query failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn repository_knowledge(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
) -> Result<Json<KnowledgeResponse>, StatusCode> {
    // Same enrolment-first check as `repository_timeline`: a repository
    // outside the caller's tenant must read exactly like one that was never
    // enrolled, not leak a 200 for a repository this tenant cannot see.
    match state
        .fleet
        .repository(&state.tenant_id, &repository_id)
        .await
    {
        Ok(Some(_)) => {}
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(error) => {
            tracing::error!(%error, "Bridge repository knowledge lookup failed");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    match state
        .knowledge
        .recall(&state.tenant_id, &repository_id, None, KNOWLEDGE_LIMIT)
        .await
    {
        Ok(result) => Ok(Json(KnowledgeResponse {
            entries: result
                .entries
                .into_iter()
                .map(KnowledgeEntrySummary::from)
                .collect(),
        })),
        Err(error) => {
            tracing::error!(%error, "Bridge repository knowledge query failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ackplane_bridge::BridgeConfig;
use ackplane_server::claim_store::{
    ClaimLeaseOutcome, ClaimRecoverRequest, ClaimStore, ClaimStoreError,
};
use ackplane_server::fleet::{
    escape_like_pattern, ActiveWorkItem, FleetFilter, FleetPage, FleetRepository, FleetSort,
    FleetSortField, FleetStore, FleetWorkFilter, FleetWorkItem, FleetWorkPage, FleetWorkSort,
    FleetWorkSortField, RepositoryDetail, RepositoryFreshness, SigningKeyStatus, SortDirection,
    TimelineEvent,
};
use ackplane_server::knowledge_store::{ActiveKnowledge, KnowledgeStore};
use ackplane_server::readiness::{
    ReadinessPage, ReadinessStatus, ReadinessStore, RepositoryReadiness,
};
use ackplane_server::signing_keys::KeyResolution;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

const FLEET_PAGE: &str = include_str!("../static/index.html");

#[derive(Clone)]
struct AppState {
    fleet: Arc<FleetStore>,
    knowledge: Arc<KnowledgeStore>,
    claims: Arc<Mutex<ClaimStore>>,
    readiness: Arc<ReadinessStore>,
    tenant_id: Arc<str>,
}

#[derive(Serialize)]
struct FleetResponse {
    repositories: Vec<FleetSummary>,
    total: i64,
    page: i64,
    page_size: i64,
}

/// `GET /api/v1/fleet` query parameters (ADR-0112). All optional.
#[derive(Deserialize)]
struct FleetQuery {
    q: Option<String>,
    freshness: Option<String>,
    coordination: Option<String>,
    sort: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

const DEFAULT_FLEET_PAGE_SIZE: i64 = 20;
const MAX_FLEET_PAGE_SIZE: i64 = 100;
const FLEET_FRESHNESS_VALUES: &[&str] = &["never_projected", "lagging", "fresh"];
const FLEET_COORDINATION_VALUES: &[&str] = &["active", "none"];

/// Parse `field:asc`/`field:desc` against the allow-listed sort fields
/// (ADR-0112). `None` (no `sort` param) is the existing default order,
/// alphabetical by repository id; anything unrecognised or malformed is a
/// `400`, never a silently-ignored value.
fn parse_fleet_sort(raw: Option<&str>) -> Result<FleetSort, StatusCode> {
    let Some(raw) = raw else {
        return Ok(FleetSort::default_order());
    };
    let (field_part, direction_part) = raw.split_once(':').ok_or(StatusCode::BAD_REQUEST)?;
    let field = match field_part {
        "repository_id" => FleetSortField::RepositoryId,
        "active_node_count" => FleetSortField::ActiveNodeCount,
        "last_activated_at" => FleetSortField::LastActivatedAt,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let direction = match direction_part {
        "asc" => SortDirection::Ascending,
        "desc" => SortDirection::Descending,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    Ok(FleetSort { field, direction })
}

/// Reject a `freshness`/`coordination` value outside its own allow-list with
/// a `400` rather than letting it reach `FleetStore::repositories`, where an
/// unrecognised value would otherwise match no row rather than error.
fn validate_allow_listed<'a>(
    value: &'a Option<String>,
    allowed: &[&str],
) -> Result<Option<&'a str>, StatusCode> {
    match value {
        None => Ok(None),
        Some(value) if allowed.contains(&value.as_str()) => Ok(Some(value.as_str())),
        Some(_) => Err(StatusCode::BAD_REQUEST),
    }
}

/// `GET /api/v1/agents` query parameters (ADR-0105 decision 5). All optional.
#[derive(Deserialize)]
struct AgentsQuery {
    repository_id: Option<String>,
    owner_id: Option<String>,
    sort: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

const DEFAULT_AGENTS_PAGE_SIZE: i64 = 20;
const MAX_AGENTS_PAGE_SIZE: i64 = 100;

/// Parse `field:asc`/`field:desc` against the allow-listed Agents sort
/// fields (ADR-0105 decision 5). `None` (no `sort` param) is the default
/// order, soonest-expiring lease first; anything unrecognised or malformed
/// is a `400`, never a silently-ignored value.
fn parse_agents_sort(raw: Option<&str>) -> Result<FleetWorkSort, StatusCode> {
    let Some(raw) = raw else {
        return Ok(FleetWorkSort::default_order());
    };
    let (field_part, direction_part) = raw.split_once(':').ok_or(StatusCode::BAD_REQUEST)?;
    let field = match field_part {
        "lease_expires_at" => FleetWorkSortField::LeaseExpiresAt,
        "repository_id" => FleetWorkSortField::RepositoryId,
        "owner_id" => FleetWorkSortField::OwnerId,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    let direction = match direction_part {
        "asc" => SortDirection::Ascending,
        "desc" => SortDirection::Descending,
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    Ok(FleetWorkSort { field, direction })
}

#[derive(Serialize)]
struct AgentsResponse {
    items: Vec<AgentWorkSummary>,
    total: i64,
    page: i64,
    page_size: i64,
}

#[derive(Serialize)]
struct AgentWorkSummary {
    repository_id: String,
    task_id: String,
    owner_id: String,
    branch: String,
    lease_expires_at_seconds: Option<u64>,
    paths: Vec<String>,
    symbols: Vec<String>,
}

impl From<FleetWorkItem> for AgentWorkSummary {
    fn from(item: FleetWorkItem) -> Self {
        Self {
            repository_id: item.repository_id,
            task_id: item.task_id,
            owner_id: item.owner_id,
            branch: item.branch,
            lease_expires_at_seconds: unix_seconds(item.lease_expires_at),
            paths: item.paths,
            symbols: item.symbols,
        }
    }
}

/// `GET /api/v1/readiness` query parameters (ADR-0105 decision 6). All
/// optional; a first slice needs no filter or sort, unlike Fleet/Agents.
#[derive(Deserialize)]
struct ReadinessQuery {
    page: Option<i64>,
    page_size: Option<i64>,
}

const DEFAULT_READINESS_PAGE_SIZE: i64 = 20;
const MAX_READINESS_PAGE_SIZE: i64 = 100;

#[derive(Serialize)]
struct ReadinessResponse {
    items: Vec<RepositoryReadinessSummary>,
    total: i64,
    page: i64,
    page_size: i64,
}

#[derive(Serialize)]
struct RepositoryReadinessSummary {
    repository_id: String,
    active_node_count: i64,
    freshness: &'static str,
    active_claim_count: i64,
    soonest_lease_expires_at_seconds: Option<u64>,
    signing_keys_resolved: i64,
    signing_keys_needing_attention: i64,
    status: &'static str,
}

/// The status word the Readiness UI badges on; distinct from
/// `freshness_label` since it names the OVERALL judgment, not just the
/// projection state one of its inputs.
fn readiness_status_label(status: ReadinessStatus) -> &'static str {
    match status {
        ReadinessStatus::Ready => "ready",
        ReadinessStatus::AttentionNeeded => "attention_needed",
        ReadinessStatus::NotReady => "not_ready",
    }
}

impl From<RepositoryReadiness> for RepositoryReadinessSummary {
    fn from(item: RepositoryReadiness) -> Self {
        Self {
            repository_id: item.repository_id,
            active_node_count: item.active_node_count,
            freshness: freshness_label(item.freshness),
            active_claim_count: item.active_claim_count,
            soonest_lease_expires_at_seconds: item.soonest_lease_expires_at.and_then(unix_seconds),
            signing_keys_resolved: item.signing_keys_resolved,
            signing_keys_needing_attention: item.signing_keys_needing_attention,
            status: readiness_status_label(item.status),
        }
    }
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

#[derive(Deserialize)]
struct RecoverClaimRequest {
    owner_id: String,
    reason: String,
    branch: String,
    lease_seconds: u64,
}

#[derive(Serialize)]
struct RecoverClaimResponse {
    task_id: String,
    owner_id: String,
    branch: String,
    claim_started_at_seconds: Option<u64>,
    lease_expires_at_seconds: Option<u64>,
    claim_lapses: u64,
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
    let claim_store = match ClaimStore::connect(config.database_url()).await {
        Ok(claims) => Arc::new(Mutex::new(claims)),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane claim delegation: {error}");
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
    let state = AppState {
        fleet: fleet_store,
        knowledge: knowledge_store,
        claims: claim_store,
        readiness: readiness_store,
        tenant_id: Arc::from(config.development_tenant_token),
    };
    let application = Router::new()
        .route("/", get(fleet_page))
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
            "/api/v1/repositories/:repository_id/signing-keys",
            get(repository_signing_keys),
        )
        .route(
            "/api/v1/repositories/:repository_id/knowledge",
            get(repository_knowledge),
        )
        .route(
            "/api/v1/repositories/:repository_id/tasks/:task_id/recover",
            post(repository_recover_claim),
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

async fn fleet(
    State(state): State<AppState>,
    Query(query): Query<FleetQuery>,
) -> Result<Json<FleetResponse>, StatusCode> {
    let sort = parse_fleet_sort(query.sort.as_deref())?;
    let freshness = validate_allow_listed(&query.freshness, FLEET_FRESHNESS_VALUES)?;
    let coordination = validate_allow_listed(&query.coordination, FLEET_COORDINATION_VALUES)?;
    let page = query.page.unwrap_or(1);
    if page < 1 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let page_size = query
        .page_size
        .unwrap_or(DEFAULT_FLEET_PAGE_SIZE)
        .clamp(1, MAX_FLEET_PAGE_SIZE);
    let escaped_q = query.q.as_deref().map(escape_like_pattern);
    let filter = FleetFilter {
        q: escaped_q.as_deref(),
        freshness,
        coordination,
    };

    state
        .fleet
        .repositories(&state.tenant_id, filter, sort, page, page_size)
        .await
        .map(
            |FleetPage {
                 repositories,
                 total,
             }| {
                Json(FleetResponse {
                    repositories: repositories.into_iter().map(FleetSummary::from).collect(),
                    total,
                    page,
                    page_size,
                })
            },
        )
        .map_err(|error| {
            tracing::error!(%error, "Bridge Fleet query failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn fleet_page() -> impl IntoResponse {
    Html(FLEET_PAGE)
}

async fn agents(
    State(state): State<AppState>,
    Query(query): Query<AgentsQuery>,
) -> Result<Json<AgentsResponse>, StatusCode> {
    let sort = parse_agents_sort(query.sort.as_deref())?;
    let page = query.page.unwrap_or(1);
    if page < 1 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let page_size = query
        .page_size
        .unwrap_or(DEFAULT_AGENTS_PAGE_SIZE)
        .clamp(1, MAX_AGENTS_PAGE_SIZE);
    let escaped_repository_id = query.repository_id.as_deref().map(escape_like_pattern);
    let escaped_owner_id = query.owner_id.as_deref().map(escape_like_pattern);
    let filter = FleetWorkFilter {
        repository_id: escaped_repository_id.as_deref(),
        owner_id: escaped_owner_id.as_deref(),
    };

    state
        .fleet
        .fleet_work(
            &state.tenant_id,
            filter,
            sort,
            page,
            page_size,
            SystemTime::now(),
        )
        .await
        .map(|FleetWorkPage { items, total }| {
            Json(AgentsResponse {
                items: items.into_iter().map(AgentWorkSummary::from).collect(),
                total,
                page,
                page_size,
            })
        })
        .map_err(|error| {
            tracing::error!(%error, "Bridge Agents query failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })
}

async fn readiness(
    State(state): State<AppState>,
    Query(query): Query<ReadinessQuery>,
) -> Result<Json<ReadinessResponse>, StatusCode> {
    let page = query.page.unwrap_or(1);
    if page < 1 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let page_size = query
        .page_size
        .unwrap_or(DEFAULT_READINESS_PAGE_SIZE)
        .clamp(1, MAX_READINESS_PAGE_SIZE);

    state
        .readiness
        .readiness(&state.tenant_id, page, page_size, SystemTime::now())
        .await
        .map(|ReadinessPage { items, total }| {
            Json(ReadinessResponse {
                items: items
                    .into_iter()
                    .map(RepositoryReadinessSummary::from)
                    .collect(),
                total,
                page,
                page_size,
            })
        })
        .map_err(|error| {
            tracing::error!(%error, "Bridge Readiness query failed");
            StatusCode::INTERNAL_SERVER_ERROR
        })
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

async fn repository_recover_claim(
    State(state): State<AppState>,
    Path((repository_id, task_id)): Path<(String, String)>,
    Json(request): Json<RecoverClaimRequest>,
) -> Result<Json<RecoverClaimResponse>, StatusCode> {
    // Same enrolment-first check every other Bridge route already follows:
    // a repository outside the caller's tenant reads exactly like one that
    // was never enrolled.
    match state
        .fleet
        .repository(&state.tenant_id, &repository_id)
        .await
    {
        Ok(Some(_)) => {}
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(error) => {
            tracing::error!(%error, "Bridge repository lookup before claim recovery failed");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // The caller names who should hold the claim next, not who holds it now -
    // the handler derives that itself, the same way a human today reads
    // `owner` off Lodestar's own overlap view before deciding to recover.
    let current = match state
        .fleet
        .claim_owner(&state.tenant_id, &repository_id, &task_id)
        .await
    {
        Ok(Some(claim)) => claim,
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(error) => {
            tracing::error!(%error, "Bridge claim-owner lookup before recovery failed");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let recover_request = ClaimRecoverRequest {
        tenant_id: state.tenant_id.to_string(),
        repository_id,
        task_id,
        expected_owner: current.owner_id,
        owner_id: request.owner_id,
        reason: request.reason,
        branch: request.branch,
        lease: Duration::from_secs(request.lease_seconds),
        paths: current.paths,
        symbols: current.symbols,
    };

    let mut claims = state.claims.lock().await;
    match claims.recover(&recover_request, SystemTime::now()).await {
        Ok(result) if result.outcome == ClaimLeaseOutcome::Granted => {
            Ok(Json(RecoverClaimResponse {
                task_id: recover_request.task_id,
                owner_id: result.owner_id,
                branch: result.branch,
                claim_started_at_seconds: unix_seconds(result.claim_started_at),
                lease_expires_at_seconds: unix_seconds(result.lease_expires_at),
                claim_lapses: result.claim_lapses,
            }))
        }
        // Rejected means the owner changed concurrently or the lease is
        // still live - ClaimStore::recover's own unconditional expiry check
        // (ADR-0111), not a judgment Bridge makes itself.
        Ok(_rejected) => Err(StatusCode::CONFLICT),
        Err(ClaimStoreError::MissingReason | ClaimStoreError::InvalidLease) => {
            Err(StatusCode::BAD_REQUEST)
        }
        Err(error) => {
            tracing::error!(%error, "Bridge claim recovery failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_fleet_sort_defaults_to_repository_id_ascending_when_absent() {
        assert_eq!(
            parse_fleet_sort(None).expect("default sort"),
            FleetSort {
                field: FleetSortField::RepositoryId,
                direction: SortDirection::Ascending,
            }
        );
    }

    #[test]
    fn parse_fleet_sort_accepts_every_allow_listed_field_and_direction() {
        let cases = [
            (
                "repository_id:asc",
                FleetSortField::RepositoryId,
                SortDirection::Ascending,
            ),
            (
                "repository_id:desc",
                FleetSortField::RepositoryId,
                SortDirection::Descending,
            ),
            (
                "active_node_count:asc",
                FleetSortField::ActiveNodeCount,
                SortDirection::Ascending,
            ),
            (
                "active_node_count:desc",
                FleetSortField::ActiveNodeCount,
                SortDirection::Descending,
            ),
            (
                "last_activated_at:asc",
                FleetSortField::LastActivatedAt,
                SortDirection::Ascending,
            ),
            (
                "last_activated_at:desc",
                FleetSortField::LastActivatedAt,
                SortDirection::Descending,
            ),
        ];
        for (raw, field, direction) in cases {
            assert_eq!(
                parse_fleet_sort(Some(raw)).unwrap_or_else(|_| panic!("{raw} must parse")),
                FleetSort { field, direction },
                "parsing {raw}"
            );
        }
    }

    #[test]
    fn parse_fleet_sort_rejects_an_unrecognised_field() {
        assert_eq!(
            parse_fleet_sort(Some("nonexistent_column:asc")),
            Err(StatusCode::BAD_REQUEST)
        );
    }

    #[test]
    fn parse_fleet_sort_rejects_an_unrecognised_direction() {
        assert_eq!(
            parse_fleet_sort(Some("repository_id:sideways")),
            Err(StatusCode::BAD_REQUEST)
        );
    }

    #[test]
    fn parse_fleet_sort_rejects_a_value_with_no_colon() {
        assert_eq!(
            parse_fleet_sort(Some("repository_id")),
            Err(StatusCode::BAD_REQUEST)
        );
    }

    #[test]
    fn validate_allow_listed_accepts_an_absent_value() {
        assert_eq!(
            validate_allow_listed(&None, FLEET_FRESHNESS_VALUES),
            Ok(None)
        );
    }

    #[test]
    fn validate_allow_listed_accepts_every_allow_listed_value() {
        for value in FLEET_FRESHNESS_VALUES {
            assert_eq!(
                validate_allow_listed(&Some((*value).to_string()), FLEET_FRESHNESS_VALUES),
                Ok(Some(*value))
            );
        }
    }

    #[test]
    fn validate_allow_listed_rejects_a_value_outside_the_list() {
        assert_eq!(
            validate_allow_listed(&Some("bogus".to_string()), FLEET_COORDINATION_VALUES),
            Err(StatusCode::BAD_REQUEST)
        );
    }

    #[test]
    fn parse_agents_sort_defaults_to_lease_expires_at_ascending_when_absent() {
        assert_eq!(
            parse_agents_sort(None).expect("default sort"),
            FleetWorkSort {
                field: FleetWorkSortField::LeaseExpiresAt,
                direction: SortDirection::Ascending,
            }
        );
    }

    #[test]
    fn parse_agents_sort_accepts_every_allow_listed_field_and_direction() {
        let cases = [
            (
                "lease_expires_at:asc",
                FleetWorkSortField::LeaseExpiresAt,
                SortDirection::Ascending,
            ),
            (
                "lease_expires_at:desc",
                FleetWorkSortField::LeaseExpiresAt,
                SortDirection::Descending,
            ),
            (
                "repository_id:asc",
                FleetWorkSortField::RepositoryId,
                SortDirection::Ascending,
            ),
            (
                "repository_id:desc",
                FleetWorkSortField::RepositoryId,
                SortDirection::Descending,
            ),
            (
                "owner_id:asc",
                FleetWorkSortField::OwnerId,
                SortDirection::Ascending,
            ),
            (
                "owner_id:desc",
                FleetWorkSortField::OwnerId,
                SortDirection::Descending,
            ),
        ];
        for (raw, field, direction) in cases {
            assert_eq!(
                parse_agents_sort(Some(raw)).unwrap_or_else(|_| panic!("{raw} must parse")),
                FleetWorkSort { field, direction },
                "parsing {raw}"
            );
        }
    }

    #[test]
    fn parse_agents_sort_rejects_an_unrecognised_field() {
        assert_eq!(
            parse_agents_sort(Some("nonexistent_column:asc")),
            Err(StatusCode::BAD_REQUEST)
        );
    }

    #[test]
    fn parse_agents_sort_rejects_an_unrecognised_direction() {
        assert_eq!(
            parse_agents_sort(Some("repository_id:sideways")),
            Err(StatusCode::BAD_REQUEST)
        );
    }

    #[test]
    fn parse_agents_sort_rejects_a_value_with_no_colon() {
        assert_eq!(
            parse_agents_sort(Some("repository_id")),
            Err(StatusCode::BAD_REQUEST)
        );
    }
}

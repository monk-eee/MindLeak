use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ackplane_bridge::BridgeConfig;
use ackplane_server::claim_store::{
    ClaimLeaseOutcome, ClaimRecoverRequest, ClaimStore, ClaimStoreError,
};
use ackplane_server::constitution_store::{ActiveConstitution, ClauseSnapshot, ConstitutionStore};
use ackplane_server::fleet::{
    escape_like_pattern, ActiveWorkItem, FleetFilter, FleetPage, FleetRepository, FleetSort,
    FleetSortField, FleetStore, FleetWorkFilter, FleetWorkItem, FleetWorkPage, FleetWorkSort,
    FleetWorkSortField, RepositoryDetail, RepositoryFreshness, SigningKeyStatus, SortDirection,
    TimelineEvent,
};
use ackplane_server::knowledge_store::{ActiveKnowledge, KnowledgeStore};
use ackplane_server::projection::{BoundedNeighborhood, ProjectedNode, Projector};
use ackplane_server::readiness::{
    ReadinessPage, ReadinessStatus, ReadinessStore, RepositoryReadiness,
};
use ackplane_server::signing_keys::KeyResolution;
use ackplane_server::telemetry_store::{ReadTelemetryRequest, TelemetryMetric, TelemetryStore};
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
    constitution: Arc<ConstitutionStore>,
    claims: Arc<Mutex<ClaimStore>>,
    projector: Arc<Projector>,
    readiness: Arc<ReadinessStore>,
    telemetry: Arc<TelemetryStore>,
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

#[derive(Serialize)]
struct StrandedClaimsResponse {
    claims: Vec<ActiveWorkSummary>,
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

/// `GET /api/v1/repositories/:repository_id/graph` query parameters
/// (ADR-0087's existing `bounded_neighborhood`, wired up for the first
/// time). All optional: an absent `seeds` falls back to
/// `Projector::sample_nodes`, the most recently touched nodes.
#[derive(Deserialize)]
struct GraphQuery {
    seeds: Option<String>,
    depth: Option<i32>,
    max_nodes: Option<i32>,
    max_fanout: Option<i32>,
}

const DEFAULT_GRAPH_DEPTH: i32 = 2;
const MAX_GRAPH_DEPTH: i32 = 4;
const DEFAULT_GRAPH_MAX_NODES: i32 = 75;
const MAX_GRAPH_MAX_NODES: i32 = 300;
const DEFAULT_GRAPH_MAX_FANOUT: i32 = 12;
const MAX_GRAPH_MAX_FANOUT: i32 = 30;
/// How many nodes `Projector::sample_nodes` seeds the view with when the
/// caller has not chosen a seed yet.
const DEFAULT_SEED_SAMPLE: i64 = 8;

#[derive(Serialize)]
struct GraphResponse {
    nodes: Vec<GraphNodeSummary>,
    edges: Vec<GraphEdgeSummary>,
    projection_stream_position: Option<i64>,
    projected_at_seconds: Option<u64>,
}

#[derive(Serialize)]
struct GraphNodeSummary {
    node_id: String,
    node_type: String,
    label: String,
    depth: i32,
}

impl From<ProjectedNode> for GraphNodeSummary {
    fn from(node: ProjectedNode) -> Self {
        Self {
            node_id: node.node_id,
            node_type: node.node_type,
            label: node.label,
            depth: node.depth,
        }
    }
}

#[derive(Serialize)]
struct GraphEdgeSummary {
    source_id: String,
    target_id: String,
    relation: String,
    effective_weight: f64,
}

/// `W_eff = W_base * 2^(-Δt_hours / half_life)`, mirroring
/// `mindleak_core::decay::effective_weight` and the SQL this same formula
/// already runs as inside `bounded_neighborhood`'s own frontier ordering --
/// computed here only for display, never stored. Clamps `elapsed <= 0` to
/// `base_weight` directly, the same guard the Rust reference implementation
/// and `knowledge_store`'s `EFFECTIVE_WEIGHT_SQL` both apply.
fn effective_weight(
    base_weight: f64,
    half_life_hours: f64,
    updated_at: SystemTime,
    now: SystemTime,
) -> f64 {
    let elapsed_hours = match now.duration_since(updated_at) {
        Ok(elapsed) if !elapsed.is_zero() => elapsed.as_secs_f64() / 3600.0,
        _ => return base_weight,
    };
    base_weight * 2.0_f64.powf(-elapsed_hours / half_life_hours)
}

#[derive(Serialize)]
struct ConstitutionResponse {
    found: bool,
    version_id: Option<String>,
    version: Option<i64>,
    status: Option<String>,
    published_at_seconds: Option<u64>,
    clauses: Vec<ConstitutionClauseSummary>,
}

#[derive(Serialize)]
struct ConstitutionClauseSummary {
    id: String,
    slug: String,
    kind: String,
    title: String,
    statement: String,
    status: String,
    consequence: Option<String>,
    scope: Option<String>,
    rationale: Option<String>,
}

impl From<ClauseSnapshot> for ConstitutionClauseSummary {
    fn from(clause: ClauseSnapshot) -> Self {
        Self {
            id: clause.id,
            slug: clause.slug,
            kind: clause.kind,
            title: clause.title,
            statement: clause.statement,
            status: clause.status,
            consequence: clause.consequence,
            scope: clause.scope,
            rationale: clause.rationale,
        }
    }
}

impl From<ActiveConstitution> for ConstitutionResponse {
    fn from(active: ActiveConstitution) -> Self {
        Self {
            found: true,
            version_id: Some(active.version_id),
            version: Some(active.version),
            status: Some(active.status),
            published_at_seconds: unix_seconds(active.published_at),
            clauses: active
                .clauses
                .into_iter()
                .map(ConstitutionClauseSummary::from)
                .collect(),
        }
    }
}

impl ConstitutionResponse {
    fn not_found() -> Self {
        Self {
            found: false,
            version_id: None,
            version: None,
            status: None,
            published_at_seconds: None,
            clauses: Vec::new(),
        }
    }
}

#[derive(Serialize)]
struct TelemetryResponse {
    metrics: Vec<TelemetryMetricSummary>,
}

/// Current health per (kind, name) -- derived from the most recent
/// success/error, distinct from the lifetime `calls`/`errors` counts also
/// reported, so a resolved past error stops reading as an active fault the
/// moment a later call succeeds (mirrors mindleak-core's own local
/// NameMetric/currently_failing logic, ADR-0010).
#[derive(Serialize)]
struct TelemetryMetricSummary {
    kind: i16,
    name: String,
    calls: i64,
    errors: i64,
    currently_failing: bool,
    last_success_at_seconds: Option<u64>,
    last_error_at_seconds: Option<u64>,
    average_duration_ms: f64,
}

impl From<TelemetryMetric> for TelemetryMetricSummary {
    fn from(metric: TelemetryMetric) -> Self {
        Self {
            kind: metric.kind,
            name: metric.name,
            calls: metric.calls,
            errors: metric.errors,
            currently_failing: metric.currently_failing,
            last_success_at_seconds: metric.last_success_at.and_then(unix_seconds),
            last_error_at_seconds: metric.last_error_at.and_then(unix_seconds),
            average_duration_ms: metric.average_duration_ms,
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
    let state = AppState {
        fleet: fleet_store,
        knowledge: knowledge_store,
        constitution: constitution_store,
        claims: claim_store,
        projector,
        readiness: readiness_store,
        telemetry: telemetry_store,
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

// The complement of `repository_claims`: what `repository_recover_claim`
// needs an operator to already know (ADR-0111 left no way to discover a
// stranded task id other than already holding it).
async fn repository_stranded_claims(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
) -> Result<Json<StrandedClaimsResponse>, StatusCode> {
    match state
        .fleet
        .stranded_claims(
            &state.tenant_id,
            &repository_id,
            SystemTime::now(),
            ACTIVE_WORK_LIMIT,
        )
        .await
    {
        Ok(Some(claims)) => Ok(Json(StrandedClaimsResponse {
            claims: claims.into_iter().map(ActiveWorkSummary::from).collect(),
        })),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(error) => {
            tracing::error!(%error, "Bridge repository stranded-claims query failed");
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

async fn repository_graph(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
    Query(query): Query<GraphQuery>,
) -> Result<Json<GraphResponse>, StatusCode> {
    // Same enrolment-first check as `repository_knowledge`: a repository
    // outside the caller's tenant reads exactly like one that was never
    // enrolled.
    match state
        .fleet
        .repository(&state.tenant_id, &repository_id)
        .await
    {
        Ok(Some(_)) => {}
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(error) => {
            tracing::error!(%error, "Bridge repository lookup before graph query failed");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    let depth = query
        .depth
        .unwrap_or(DEFAULT_GRAPH_DEPTH)
        .clamp(1, MAX_GRAPH_DEPTH);
    let max_nodes = query
        .max_nodes
        .unwrap_or(DEFAULT_GRAPH_MAX_NODES)
        .clamp(1, MAX_GRAPH_MAX_NODES);
    let max_fanout = query
        .max_fanout
        .unwrap_or(DEFAULT_GRAPH_MAX_FANOUT)
        .clamp(1, MAX_GRAPH_MAX_FANOUT);

    let seeds: Vec<String> = match query.seeds.as_deref() {
        Some(raw) if !raw.trim().is_empty() => raw
            .split(',')
            .map(str::trim)
            .filter(|seed| !seed.is_empty())
            .map(str::to_string)
            .collect(),
        _ => {
            match state
                .projector
                .sample_nodes(&state.tenant_id, &repository_id, DEFAULT_SEED_SAMPLE)
                .await
            {
                Ok(nodes) => nodes.into_iter().map(|node| node.node_id).collect(),
                Err(error) => {
                    tracing::error!(%error, "Bridge default graph seed sampling failed");
                    return Err(StatusCode::INTERNAL_SERVER_ERROR);
                }
            }
        }
    };

    // No seed and nothing to sample (never projected, or a genuinely empty
    // graph): an empty response is the honest answer, not an error.
    if seeds.is_empty() {
        return Ok(Json(GraphResponse {
            nodes: Vec::new(),
            edges: Vec::new(),
            projection_stream_position: None,
            projected_at_seconds: None,
        }));
    }

    match state
        .projector
        .bounded_neighborhood(
            &state.tenant_id,
            &repository_id,
            &seeds,
            depth,
            max_nodes,
            max_fanout,
        )
        .await
    {
        Ok(BoundedNeighborhood {
            nodes,
            edges,
            freshness,
        }) => {
            let now = SystemTime::now();
            Ok(Json(GraphResponse {
                nodes: nodes.into_iter().map(GraphNodeSummary::from).collect(),
                edges: edges
                    .into_iter()
                    .map(|edge| GraphEdgeSummary {
                        effective_weight: effective_weight(
                            edge.base_weight,
                            edge.half_life_hours,
                            edge.updated_at,
                            now,
                        ),
                        source_id: edge.source_id,
                        target_id: edge.target_id,
                        relation: edge.relation,
                    })
                    .collect(),
                projection_stream_position: freshness.as_ref().map(|f| f.stream_position),
                projected_at_seconds: freshness.and_then(|f| unix_seconds(f.projected_at)),
            }))
        }
        Err(error) => {
            tracing::error!(%error, "Bridge repository graph query failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn repository_constitution(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
) -> Result<Json<ConstitutionResponse>, StatusCode> {
    // Same enrolment-first check as `repository_knowledge`: a repository
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
            tracing::error!(%error, "Bridge repository constitution lookup failed");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    match state
        .constitution
        .get_active(&state.tenant_id, &repository_id)
        .await
    {
        Ok(Some(active)) => Ok(Json(ConstitutionResponse::from(active))),
        Ok(None) => Ok(Json(ConstitutionResponse::not_found())),
        Err(error) => {
            tracing::error!(%error, "Bridge repository constitution query failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn repository_telemetry(
    State(state): State<AppState>,
    Path(repository_id): Path<String>,
) -> Result<Json<TelemetryResponse>, StatusCode> {
    // Same enrolment-first check every other Bridge route already follows: a
    // repository outside the caller's tenant reads exactly like one that was
    // never enrolled.
    match state
        .fleet
        .repository(&state.tenant_id, &repository_id)
        .await
    {
        Ok(Some(_)) => {}
        Ok(None) => return Err(StatusCode::NOT_FOUND),
        Err(error) => {
            tracing::error!(%error, "Bridge repository telemetry lookup failed");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    // Zero/None select every kind/name (the same convention TelemetryService
    // documents on the wire); series points are not rendered by this route
    // yet -- the sparkline dashboard is task:60ba293846ec's own scope.
    match state
        .telemetry
        .read(ReadTelemetryRequest {
            tenant_id: state.tenant_id.to_string(),
            repository_id,
            kind: 0,
            name: None,
            bucket_seconds: 3600,
            max_points: 1,
        })
        .await
    {
        Ok(snapshot) => Ok(Json(TelemetryResponse {
            metrics: snapshot
                .metrics
                .into_iter()
                .map(TelemetryMetricSummary::from)
                .collect(),
        })),
        Err(error) => {
            tracing::error!(%error, "Bridge repository telemetry query failed");
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

    #[test]
    fn effective_weight_is_unchanged_at_zero_elapsed_time() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        assert_eq!(effective_weight(1.0, 168.0, now, now), 1.0);
    }

    #[test]
    fn effective_weight_halves_after_one_half_life() {
        let updated_at = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let now = updated_at + Duration::from_secs(168 * 3600);
        assert!((effective_weight(1.0, 168.0, updated_at, now) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn effective_weight_clamps_a_stale_updated_at_in_the_future_to_the_base_weight() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let updated_at = now + Duration::from_secs(60);
        assert_eq!(effective_weight(1.0, 168.0, updated_at, now), 1.0);
    }
}

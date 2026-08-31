use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use ackplane_bridge::administration::{administration_routes, AdministrationApiState};
use ackplane_bridge::context_api::{context_routes, ContextApiState};
use ackplane_bridge::delegation_api::{delegation_routes, DelegationApiState};
use ackplane_bridge::design_api::{design_routes, DesignApiState};
use ackplane_bridge::evidence::BridgeEvidenceStore;
use ackplane_bridge::evidence_api::{evidence_routes, EvidenceApiState};
use ackplane_bridge::human_decision_api::{human_decision_routes, HumanDecisionApiState};
use ackplane_bridge::knowledge_api::{knowledge_routes, KnowledgeApiState};
use ackplane_bridge::live_feed::{live_feed_routes, LiveFeedApiState};
use ackplane_bridge::shared_assets::shared_asset_routes;
use ackplane_bridge::supervisor_api::{supervisor_routes, SupervisorApiState};
use ackplane_bridge::work_api::{work_routes, WorkApiState};
use ackplane_bridge::work_command_api::{work_command_routes, WorkCommandApiState};
use ackplane_bridge::BridgeConfig;
use ackplane_server::administration_store::AdministrationStore;
use ackplane_server::claim_store::ClaimStore;
use ackplane_server::constitution_store::ConstitutionStore;
use ackplane_server::context_packet_store::ContextPacketStore;
use ackplane_server::delegation_store::DelegationStore;
use ackplane_server::design_materialization_store::MaterializationStore;
use ackplane_server::design_store::DesignStore;
use ackplane_server::export_provider::ExportProviderConfig;
use ackplane_server::fleet::{FleetStore, RepositoryFreshness};
use ackplane_server::human_decision_store::HumanDecisionStore;
use ackplane_server::knowledge_store::KnowledgeStore;
use ackplane_server::live_feed_store::LiveFeedStore;
use ackplane_server::projection::Projector;
use ackplane_server::readiness::ReadinessStore;
use ackplane_server::snapshot_provider::SnapshotProviderConfig;
use ackplane_server::supervisor_store::SupervisorStore;
use ackplane_server::telemetry_store::TelemetryStore;
use ackplane_server::work_command_store::WorkCommandService;
use ackplane_server::work_store::WorkStore;
use axum::{
    response::{Html, IntoResponse},
    routing::{get, post},
    Router,
};
use tokio::sync::Mutex;

mod handlers;

use handlers::{
    agents, constitution_page, fleet, list_constitution_proposals, propose_constitution_clause,
    readiness, repository_claims, repository_constitution, repository_detail, repository_graph,
    repository_knowledge, repository_recover_claim, repository_signing_keys,
    repository_stranded_claims, repository_telemetry, repository_timeline, telemetry_page,
    withdraw_constitution_proposal,
};

const FLEET_PAGE: &str = include_str!("../static/index.html");
const AGENTS_PAGE: &str = include_str!("../static/agents.html");
const GRAPH_PAGE: &str = include_str!("../static/graph.html");

#[derive(Clone)]
struct AppState {
    fleet: Arc<FleetStore>,
    knowledge: Arc<KnowledgeStore>,
    // A plain read-only Arc until ADR-0126: propose_clause takes &mut self,
    // so the same Mutex-per-mutable-store pattern claims/ClaimStore already
    // uses applies here too.
    constitution: Arc<Mutex<ConstitutionStore>>,
    claims: Arc<ClaimStore>,
    projector: Arc<Projector>,
    readiness: Arc<ReadinessStore>,
    telemetry: Arc<TelemetryStore>,
    work: Arc<WorkStore>,
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
    // One pool for this process (ADR-0143 decision 1); every migrated store
    // takes a clone of this handle. Stores still on `connect(database_url)`
    // keep their own connection until their turn in the migration sequence.
    let db_pool = match ackplane_server::db_pool::build_pool(
        config.database_url(),
        ackplane_server::db_pool::SERVICE_POOL_MAX_SIZE,
    ) {
        Ok(pool) => pool,
        Err(error) => {
            eprintln!("ackplane-bridge: could not build the Ackplane database pool: {error}");
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
    let knowledge_store = match KnowledgeStore::connect(&db_pool).await {
        Ok(knowledge) => Arc::new(knowledge),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane knowledge domain: {error}");
            return;
        }
    };
    let constitution_store = match ConstitutionStore::connect(config.database_url()).await {
        Ok(constitution) => Arc::new(Mutex::new(constitution)),
        Err(error) => {
            eprintln!(
                "ackplane-bridge: could not connect to Ackplane constitution domain: {error}"
            );
            return;
        }
    };
    let claim_store = match ClaimStore::connect(&db_pool).await {
        // No `Mutex`: ADR-0111 wrapped this one because `ClaimStore`'s
        // mutating methods took `&mut self`, and ADR-0143 retired that
        // reason. The CAS row lock in the database is what serialises a
        // claim, so a process-wide lock only removed the concurrency the
        // pool exists to allow.
        Ok(claims) => Arc::new(claims),
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
    let readiness_store = match ReadinessStore::connect(&db_pool).await {
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
    let supervisor_store = match SupervisorStore::connect(config.database_url()).await {
        Ok(store) => Arc::new(store),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane supervisor store: {error}");
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
    let live_feed_store = match LiveFeedStore::connect(&db_pool).await {
        Ok(live_feed) => Arc::new(live_feed),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane live feed: {error}");
            return;
        }
    };
    let human_decision_store = match HumanDecisionStore::connect(&db_pool).await {
        Ok(decisions) => Arc::new(decisions),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane human decisions: {error}");
            return;
        }
    };
    let delegation_store = match DelegationStore::connect(&db_pool).await {
        Ok(delegations) => Arc::new(delegations),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane delegations: {error}");
            return;
        }
    };
    let work_store = match WorkStore::connect(config.database_url()).await {
        Ok(work) => Arc::new(work),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane's Work domain: {error}");
            return;
        }
    };
    let work_command_service = match WorkCommandService::connect(config.database_url()).await {
        Ok(commands) => Arc::new(Mutex::new(commands)),
        Err(error) => {
            eprintln!(
                "ackplane-bridge: could not connect to Ackplane's Work command domain: {error}"
            );
            return;
        }
    };
    let design_store = match DesignStore::connect(config.database_url()).await {
        Ok(design) => Arc::new(Mutex::new(design)),
        Err(error) => {
            eprintln!("ackplane-bridge: could not connect to Ackplane's Design domain: {error}");
            return;
        }
    };
    let design_materialization_store = match MaterializationStore::connect(config.database_url())
        .await
    {
        Ok(materializations) => Arc::new(Mutex::new(materializations)),
        Err(error) => {
            eprintln!(
                "ackplane-bridge: could not connect to Ackplane's Design materialization domain: {error}"
            );
            return;
        }
    };
    let administration_store = match AdministrationStore::connect(config.database_url()).await {
        Ok(administration) => Arc::new(Mutex::new(administration)),
        Err(error) => {
            eprintln!(
                "ackplane-bridge: could not connect to Ackplane's Administration domain: {error}"
            );
            return;
        }
    };
    // `None` (Snapshot reports `unavailable`) unless an operator has opted in
    // by setting `ACKPLANE_SNAPSHOT_DIR` -- resolved, never guessed, the same
    // rule `BridgeConfig::resolve` already applies to its own settings.
    let snapshot_config = SnapshotProviderConfig::resolve(
        |key| std::env::var(key).ok(),
        config.database_url().to_string(),
    )
    .map(Arc::new);
    // Same "refuse, never guess" rule as Snapshot: `None` unless an operator
    // has set `ACKPLANE_EXPORT_DIR`.
    let export_config = ExportProviderConfig::resolve(
        |key| std::env::var(key).ok(),
        config.database_url().to_string(),
    )
    .map(Arc::new);
    let tenant_id: Arc<str> = Arc::from(config.development_tenant_token.clone());
    let evidence_api_state =
        EvidenceApiState::new(evidence_store, fleet_store.clone(), tenant_id.clone());
    let supervisor_api_state =
        SupervisorApiState::new(supervisor_store, fleet_store.clone(), tenant_id.clone());
    let knowledge_api_state = KnowledgeApiState::new(
        knowledge_store.clone(),
        fleet_store.clone(),
        tenant_id.clone(),
    );
    let context_api_state =
        ContextApiState::new(context_packet_store, fleet_store.clone(), tenant_id.clone());
    let delegation_api_state =
        DelegationApiState::new(delegation_store, fleet_store.clone(), tenant_id.clone());
    let human_decision_api_state =
        HumanDecisionApiState::new(human_decision_store, fleet_store.clone(), tenant_id.clone());
    let work_api_state =
        WorkApiState::new(work_store.clone(), fleet_store.clone(), tenant_id.clone());
    let work_command_api_state =
        WorkCommandApiState::new(work_command_service, fleet_store.clone(), tenant_id.clone());
    let administration_api_state = AdministrationApiState::with_claims(
        fleet_store.clone(),
        tenant_id.clone(),
        administration_store,
        claim_store.clone(),
        snapshot_config,
        export_config,
    );
    let live_feed_api_state =
        LiveFeedApiState::new(live_feed_store, fleet_store.clone(), tenant_id.clone());
    let design_api_state = DesignApiState::new(
        design_store,
        design_materialization_store,
        fleet_store.clone(),
        tenant_id.clone(),
    );
    let state = AppState {
        fleet: fleet_store,
        knowledge: knowledge_store,
        constitution: constitution_store,
        claims: claim_store,
        projector,
        readiness: readiness_store,
        telemetry: telemetry_store,
        work: work_store,
        tenant_id,
    };
    let application = Router::new()
        .route("/", get(fleet_page))
        .route("/agents", get(agents_page))
        .route("/graph", get(graph_page))
        .route("/telemetry", get(telemetry_page))
        .route("/constitution", get(constitution_page))
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
            "/api/v1/repositories/:repository_id/constitution/proposals",
            get(list_constitution_proposals).post(propose_constitution_clause),
        )
        .route(
            "/api/v1/repositories/:repository_id/constitution/proposals/:proposal_id/withdraw",
            post(withdraw_constitution_proposal),
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
        .merge(delegation_routes(delegation_api_state))
        .merge(human_decision_routes(human_decision_api_state))
        .merge(administration_routes(administration_api_state))
        .merge(supervisor_routes(supervisor_api_state))
        .merge(live_feed_routes(live_feed_api_state))
        .merge(work_routes(work_api_state))
        .merge(work_command_routes(work_command_api_state))
        .merge(design_routes(design_api_state))
        .merge(shared_asset_routes());
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

async fn agents_page() -> impl IntoResponse {
    Html(AGENTS_PAGE)
}

async fn graph_page() -> impl IntoResponse {
    Html(GRAPH_PAGE)
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

    #[tokio::test]
    async fn agents_page_refreshes_only_while_visible_and_links_the_fleet_agents_section() {
        let response = agents_page().await.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("reading the agents page body");
        let body = String::from_utf8(body.to_vec()).expect("agents page body is valid UTF-8");

        for required in [
            "function startVisibleRefresh(load, intervalMs)",
            "document.visibilityState === \"visible\"",
            "document.addEventListener(\"visibilitychange\"",
            "startVisibleRefresh(loadAgents, REFRESH_INTERVAL_MS)",
            "id=\"agents-search-repo\"",
            "id=\"agents-search-owner\"",
            "id=\"agents-sort\"",
            "/api/v1/agents?",
        ] {
            assert!(body.contains(required), "agents.html is missing {required}");
        }
    }

    async fn graph_page_body() -> String {
        let response = graph_page().await.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("reading the graph page body");
        String::from_utf8(body.to_vec()).expect("graph page body is valid UTF-8")
    }

    /// The controls must not offer a range wider than `repository_graph`
    /// itself accepts: it clamps depth/max_nodes/max_fanout server-side, so a
    /// control promising more would silently return a smaller answer than the
    /// operator asked for.
    #[tokio::test]
    async fn graph_page_reads_the_projection_within_the_endpoints_own_bounds() {
        let body = graph_page_body().await;

        assert!(
            body.contains("/graph`"),
            "graph.html must query the repository graph endpoint"
        );
        for (control, bound) in [
            ("depth", "max=\"4\""),
            ("max_nodes", "max=\"300\""),
            ("max_fanout", "max=\"30\""),
        ] {
            assert!(
                body.contains(bound),
                "graph.html's {control} control must stop at the endpoint's own limit ({bound})"
            );
        }
    }

    /// Decay is the point (MindLeak invariant 2), so effective weight has to
    /// reach what the operator actually sees. A page that computed it and
    /// showed it only in a tooltip would render every edge identically and
    /// hide the one property the graph exists to express.
    #[tokio::test]
    async fn graph_page_draws_decay_rather_than_only_reporting_it() {
        let body = graph_page_body().await;

        for derived in [
            "stroke-width\",(0.6+w*2.4)",
            "stroke-opacity\",(0.14+w*0.66)",
        ] {
            assert!(
                body.contains(derived),
                "graph.html must derive {derived} from an edge's effective weight"
            );
        }
    }

    /// `repository_graph` returns the projection's own stream position and
    /// rebuild time for one reason: a stale projection must be legible rather
    /// than quietly old. "Never projected" and "projected at position 0" are
    /// different answers and the page must not collapse them.
    #[tokio::test]
    async fn graph_page_makes_an_absent_or_stale_projection_legible() {
        let body = graph_page_body().await;

        for required in [
            "projection_stream_position",
            "projected_at_seconds",
            "no projection yet",
            "over a day old",
            // A one-node graph read "Showing 1 nodes, 0 edges" before this.
            "plural(",
        ] {
            assert!(body.contains(required), "graph.html is missing {required}");
        }
    }

    /// Regression: the header rendered `projection_stream_position` under the
    /// label "Ledger position". Those are different quantities — the projection
    /// checkpoint is the last structural fact it consumed, while the ledger head
    /// counts every record — so for a repository at ledger 2 / projection 1 the
    /// page displayed "Ledger position 1", naming a number it was not showing.
    ///
    /// This was unfalsifiable until projection freshness stopped conflating the
    /// two, which is why the wrong wording is banned outright rather than merely
    /// asserting the right one: a test that only checks the replacement passes
    /// happily beside a reintroduced falsehood.
    #[tokio::test]
    async fn graph_page_names_the_projection_position_rather_than_the_ledger() {
        let body = graph_page_body().await;

        assert!(
            body.contains("Projection position"),
            "graph.html must label the value it actually renders"
        );
        assert!(
            !body.contains("Ledger position"),
            "graph.html renders the projection checkpoint, not the ledger head; \
             `index.html` and `administration.html` own that label because they \
             genuinely read `ledger_stream_position`"
        );
    }

    /// The memory plane is observed here, never edited: the graph is derived
    /// from the ledger, so a mutation from this page could only ever
    /// contradict its own source.
    #[tokio::test]
    async fn graph_page_only_ever_reads() {
        let body = graph_page_body().await;

        assert_eq!(
            body.matches("fetch(").count(),
            1,
            "graph.html should make exactly one request - the graph read"
        );
        for verb in ["\"POST\"", "\"PUT\"", "\"PATCH\"", "\"DELETE\""] {
            assert!(
                !body.contains(verb),
                "graph.html must not carry the mutating verb {verb}"
            );
        }
    }

    /// A force simulation left running in a hidden tab burns a core for
    /// nobody, the same discipline the fleet page's polling already follows.
    #[tokio::test]
    async fn graph_page_stops_simulating_while_hidden() {
        let body = graph_page_body().await;

        for required in [
            "document.visibilityState!==\"visible\"",
            "document.addEventListener(\"visibilitychange\"",
        ] {
            assert!(body.contains(required), "graph.html is missing {required}");
        }
    }

    /// The legend doubles as the node-type filter, so every type a reader can
    /// see named is one they can switch off. It has to be built from real
    /// buttons carrying `aria-pressed`: a filter made of styled `div`s is
    /// unreachable by keyboard and silent to assistive tech, and the counts
    /// must follow the filter or the page reports more than it draws.
    #[tokio::test]
    async fn graph_page_filters_node_types_accessibly() {
        let body = graph_page_body().await;

        for required in [
            "aria-pressed=",
            "hiddenKinds",
            "role=\"group\" aria-label=\"Filter node types\"",
            "view.nodes.filter(visibleNode)",
        ] {
            assert!(body.contains(required), "graph.html is missing {required}");
        }
    }

    /// Every control that selects a node must be a real control. These began
    /// as `div`s with click handlers, which no keyboard can reach.
    #[tokio::test]
    async fn graph_page_selection_controls_are_focusable() {
        let body = graph_page_body().await;

        assert!(
            body.contains("<button type=\"button\" class=\"neighbour\""),
            "graph.html's neighbour rows must be buttons, not clickable divs"
        );
        assert!(
            !body.contains("<div class=\"neighbour\""),
            "graph.html must not reintroduce a non-focusable neighbour row"
        );
    }

    /// A deep link may carry some bounds and omit others. `Number(null)` is
    /// `0` rather than `NaN`, so a guard that coerces before checking presence
    /// reads an absent bound as zero and clamps that control to its minimum --
    /// which silently renders a one-node graph. Presence must be decided on
    /// the raw string.
    #[tokio::test]
    async fn graph_page_ignores_an_absent_bound_instead_of_reading_it_as_zero() {
        let body = graph_page_body().await;

        assert!(
            body.contains("const raw=params.get(name)") && body.contains("raw===null"),
            "graph.html must test the raw query parameter for presence before coercing it"
        );
        assert!(
            !body.contains("const raw=Number(params.get(name))"),
            "graph.html must not coerce a query parameter before checking it is present"
        );
    }

    #[tokio::test]
    async fn fleet_page_links_to_the_standalone_agents_dashboard() {
        let response = fleet_page().await.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("reading the fleet page body");
        let body = String::from_utf8(body.to_vec()).expect("fleet page body is valid UTF-8");

        assert!(
            body.contains("href=\"/agents\""),
            "index.html's Agents section must link to the standalone /agents dashboard"
        );
    }

    #[tokio::test]
    async fn fleet_page_follows_every_timeline_and_knowledge_cursor_in_repository_detail() {
        let response = fleet_page().await.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("reading the fleet page body");
        let body = String::from_utf8(body.to_vec()).expect("fleet page body is valid UTF-8");

        for required in [
            "async function loadTimelinePages(repositoryId)",
            "query.set(\"before\", before)",
            "page.next_before ?? null",
            "loadTimelinePages(repositoryId)",
            "async function loadKnowledgePages(repositoryId)",
            "before_confirmed_at_micros",
            "before_knowledge_id",
            "loadKnowledgePages(repositoryId)",
        ] {
            assert!(
                body.contains(required),
                "index.html must consume every timeline and knowledge keyset page: missing {required}"
            );
        }
    }

    #[tokio::test]
    async fn fleet_page_follows_every_claim_cursor_in_repository_detail() {
        let response = fleet_page().await.into_response();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("reading the fleet page body");
        let body = String::from_utf8(body.to_vec()).expect("fleet page body is valid UTF-8");

        for required in [
            "async function loadClaimPages(repositoryId, collection)",
            "while (cursor)",
            "page.next_after || null",
            "after_lease_expires_at_micros",
            "after_task_id",
            "loadClaimPages(repositoryId,\"claims\")",
            "loadClaimPages(repositoryId,\"stranded-claims\")",
        ] {
            assert!(
                body.contains(required),
                "index.html must consume every claim keyset page: missing {required}"
            );
        }
        assert!(
            !body.contains("one past the first 50 shown"),
            "Fleet detail must not tell an operator to manually supply a claim hidden by pagination"
        );
    }
}

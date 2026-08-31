//! End-to-end coverage for ADR-0116 slice 3 (task:9fb668080d71): a directive
//! issued into Ackplane's durable store is delivered over a live authenticated
//! NodeSync connection, processed by the real `SupervisorInbox`, and its
//! receipt transmitted back over the same stream and recorded server-side.
//!
//! This is the first coverage that closes ADR-0107's control loop over real
//! gRPC. Before it, `SupervisorInbox::receive` and the server's
//! `service/directive_receipt.rs` were each exercised only with synthetic
//! frames, and nothing transported a directive between them: the server never
//! emitted an `AgentDirective` frame at all, and `DirectiveStore` had no read
//! path to find one with.
//!
//! Skipped unless `ACKPLANE_TEST_DATABASE_URL` names the gated test
//! PostgreSQL database.
//!
//! Run against the compose topology with, e.g.:
//! ```text
//! docker compose up -d postgres test-db-init
//! ACKPLANE_TEST_DATABASE_URL=postgres://.../ackplane_test \
//!   cargo test -p ackplane-bridge --test supervisor_directive_end_to_end
//! ```

// Each test binary compiles this shared module separately, so the helpers the
// other binaries use look dead from in here.
#[allow(dead_code)]
mod supervisor_api_support;

use ackplane_client::{auth::SeedSigner, node_sync::NodeSyncConnection};
use ackplane_protocol::{
    supervisor::{
        directive_payload_digest, SupervisorCapabilities, SupervisorDirectiveCapability,
        SupervisorIdentity, SupervisorOutboxDurability, SupervisorRegistration, SupervisorRuntime,
        SupervisorSession, SupervisorWorkerState,
    },
    v1::{self, agent_directive, node_sync_service_server::NodeSyncServiceServer, AgentDirective},
};
use ackplane_server::{
    directive_store::DirectiveStore, ledger::LedgerStore, service::NodeSyncService,
    supervisor_store::SupervisorStore,
};
use ackplane_supervisor::SupervisorInbox;
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

use supervisor_api_support::{enroll_repository, unique_id};

const SESSION_STARTED_AT: &str = "2026-01-01T00:00:00Z";
const SESSION_STARTED_AT_SECONDS: i64 = 1_767_225_600;

struct TestServer {
    endpoint: String,
    _shutdown: oneshot::Sender<()>,
}

async fn start_sync_server(database_url: &str) -> TestServer {
    let ledger = LedgerStore::connect(database_url)
        .await
        .expect("the gated test database should accept ledger migrations");
    let supervisors = SupervisorStore::connect(database_url)
        .await
        .expect("the gated test database should accept supervisor migrations");
    let db_pool = ackplane_server::db_pool::build_pool(
        database_url,
        ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
    )
    .expect("the gated test database url builds a pool");
    let directives = DirectiveStore::connect(&db_pool)
        .await
        .expect("the gated test database should accept directive migrations");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("the test service should bind loopback");
    let address = listener.local_addr().unwrap();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    tokio::spawn(async move {
        Server::builder()
            .add_service(NodeSyncServiceServer::new(
                NodeSyncService::with_supervisor_and_directive_store(
                    ledger,
                    supervisors,
                    directives,
                    v1::FlowControl {
                        max_in_flight_batches: 16,
                        max_batch_bytes: 1_048_576,
                    },
                ),
            ))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("the test service should run");
    });

    TestServer {
        endpoint: format!("http://{address}"),
        _shutdown: shutdown_tx,
    }
}

struct EnrolledNode {
    tenant_id: String,
    repository_id: String,
    node_id: String,
    signing_key_id: String,
    seed: [u8; 32],
}

async fn enroll(database_url: &str) -> EnrolledNode {
    let unique = unique_id("slice3");
    let tenant_id = format!("tenant-{unique}");
    let repository_id = format!("repository-{unique}");
    let node_id = enroll_repository(database_url, &tenant_id, &repository_id, &unique).await;
    EnrolledNode {
        tenant_id,
        repository_id,
        node_id,
        signing_key_id: format!("signing-key-{unique}"),
        seed: Sha256::digest(format!("key-{unique}").as_bytes()).into(),
    }
}

async fn connect(server: &TestServer, node: &EnrolledNode) -> NodeSyncConnection {
    let signer = SeedSigner::new(
        node.signing_key_id.clone(),
        node.node_id.clone(),
        &node.seed,
    );
    NodeSyncConnection::open(
        &server.endpoint,
        &signer,
        &node.tenant_id,
        &node.repository_id,
        vec!["synchronize".to_string()],
        0,
    )
    .await
    .expect("an activated node should authenticate over real gRPC")
}

fn registration_frame(supervisor_id: &str, node_id: &str) -> v1::NodeFrame {
    v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::SupervisorRegistration(registration(
            supervisor_id,
            node_id,
        ))),
    }
}

fn registration(supervisor_id: &str, node_id: &str) -> v1::SupervisorRegistration {
    v1::SupervisorRegistration {
        supervisor_id: supervisor_id.to_string(),
        node_id: node_id.to_string(),
        supervisor_version: "supervisor:v1".to_string(),
        protocol_version: "v1".to_string(),
        supported_directives: vec![v1::SupervisorDirectiveCapability::Pause as i32],
        supports_checkpoint: true,
        supports_force_termination: false,
        outbox_durability: v1::SupervisorOutboxDurability::Persistent as i32,
        recoverable_outbox: true,
    }
}

fn session_wire(supervisor_id: &str, session_id: &str, worker_id: &str) -> v1::SupervisorSession {
    v1::SupervisorSession {
        supervisor_id: supervisor_id.to_string(),
        session_id: session_id.to_string(),
        worker_id: worker_id.to_string(),
        runtime: v1::SupervisorRuntime::LocalMachine as i32,
        started_at: SESSION_STARTED_AT.to_string(),
        state: v1::SupervisorWorkerState::Started as i32,
    }
}

fn session_frame(supervisor_id: &str, session_id: &str, worker_id: &str) -> v1::NodeFrame {
    v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::SupervisorSession(session_wire(
            supervisor_id,
            session_id,
            worker_id,
        ))),
    }
}

/// The local durable inbox a real supervisor would own, bound to the same
/// identity and session it just registered over the wire.
fn local_inbox(node: &EnrolledNode, supervisor_id: &str, session_id: &str) -> SupervisorInbox {
    let registration = SupervisorRegistration {
        supervisor_id: supervisor_id.to_string(),
        identity: SupervisorIdentity {
            tenant_id: node.tenant_id.clone(),
            repository_id: node.repository_id.clone(),
            node_id: node.node_id.clone(),
        },
        supervisor_version: "supervisor:v1".to_string(),
        protocol_version: "v1".to_string(),
        capabilities: SupervisorCapabilities {
            // Deliberately Pause only: the capability refusal test below
            // depends on this supervisor never having declared Prompt.
            supported_directives: vec![SupervisorDirectiveCapability::Pause],
            supports_checkpoint: true,
            supports_force_termination: false,
            outbox_durability: SupervisorOutboxDurability::Persistent,
            recoverable_outbox: true,
        },
    };
    let session = SupervisorSession {
        session_id: session_id.to_string(),
        supervisor_id: supervisor_id.to_string(),
        worker_id: "worker-1".to_string(),
        runtime: SupervisorRuntime::LocalMachine,
        started_at: SESSION_STARTED_AT_SECONDS,
        state: SupervisorWorkerState::Started,
    };
    SupervisorInbox::open_in_memory(registration, session)
        .expect("a local inbox should open for this supervisor")
}

fn pause_directive(node: &EnrolledNode, session_id: &str, directive_id: &str) -> AgentDirective {
    let expires_at = OffsetDateTime::now_utc() + time::Duration::seconds(600);
    let mut directive = AgentDirective {
        directive_id: directive_id.to_string(),
        tenant_id: node.tenant_id.clone(),
        project_id: "project:slice3".to_string(),
        repository_id: node.repository_id.clone(),
        target_node_id: node.node_id.clone(),
        target_agent_session_id: session_id.to_string(),
        kind: v1::DirectiveKind::Pause as i32,
        schema_version: "v1".to_string(),
        issuing_principal_id: "principal:operator".to_string(),
        rationale: "pause at a safe checkpoint".to_string(),
        task_id: "task:slice3".to_string(),
        goal_id: "goal:slice3".to_string(),
        context_packet_id: String::new(),
        created_at: String::new(),
        expires_at: expires_at
            .format(&Rfc3339)
            .expect("an expiry should format as RFC3339"),
        sequence: 0,
        idempotency_key: format!("{directive_id}:enqueue"),
        payload_digest: Vec::new(),
        required_capability: "pause.v1".to_string(),
        policy_refs: vec!["policy:slice3".to_string()],
        knowledge_refs: Vec::new(),
        evidence_refs: Vec::new(),
        payload: Some(agent_directive::Payload::Pause(v1::PauseDirective {
            checkpoint_required: true,
        })),
    };
    directive.payload_digest =
        directive_payload_digest(&directive).expect("a pause directive has a payload digest");
    directive
}

fn database_url() -> Option<String> {
    std::env::var("ACKPLANE_TEST_DATABASE_URL").ok()
}

/// ADR-0107's loop, closed: issue -> deliver -> process -> receipt -> record.
///
/// Every step runs against the real component. The directive goes through the
/// durable `DirectiveStore`, crosses a real authenticated gRPC stream, is
/// judged by the real `SupervisorInbox`, and its receipt returns over that
/// same stream to the real server-side recorder.
#[tokio::test]
async fn a_directive_reaches_a_live_supervisor_and_its_receipt_returns() {
    let Some(database_url) = database_url() else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let node = enroll(&database_url).await;
    let server = start_sync_server(&database_url).await;
    let supervisor_id = "supervisor-slice3";
    let session_id = "session-slice3";

    // The supervisor comes up: register, then open its session.
    let mut connection = connect(&server, &node).await;
    connection
        .exchange_supervisor_frame(registration_frame(supervisor_id, &node.node_id))
        .await
        .expect("registration should be accepted");
    connection
        .exchange_supervisor_frame(session_frame(supervisor_id, session_id, "worker-1"))
        .await
        .expect("the session should be accepted");

    // Nothing has been issued yet, so nothing is delivered. A queue that
    // invents work is worse than one that is empty.
    assert!(
        connection.next_directive().is_none(),
        "no directive was issued, so none may be delivered"
    );

    // An operator issues a directive through the durable store.
    let directive_pool = ackplane_server::db_pool::build_pool(
        &database_url,
        ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
    )
    .expect("the gated test database url builds a pool");
    let directives = DirectiveStore::connect(&directive_pool)
        .await
        .expect("the directive store should connect");
    directives
        .enqueue(pause_directive(&node, session_id, "directive-slice3-1"))
        .await
        .expect("the directive should be enqueued");

    // The supervisor re-announces its session, which is what a live supervisor
    // does on reconnect, and the directive is delivered on that connection.
    let mut connection = connect(&server, &node).await;
    connection
        .exchange_supervisor_frame(registration_frame(supervisor_id, &node.node_id))
        .await
        .expect("registration should replay");
    connection
        .exchange_supervisor_frame(session_frame(supervisor_id, session_id, "worker-1"))
        .await
        .expect("the session should replay");

    let delivered = connection
        .next_directive()
        .expect("the pending directive should be delivered over the live connection");
    assert_eq!(delivered.directive_id, "directive-slice3-1");
    assert_eq!(delivered.target_agent_session_id, session_id);

    // The real local inbox judges it and produces the durable receipt.
    let inbox = local_inbox(&node, supervisor_id, session_id);
    let receipt = inbox
        .receive(&delivered, OffsetDateTime::now_utc())
        .expect("a directive naming a declared capability should be accepted");
    assert_eq!(receipt.directive_id, "directive-slice3-1");

    // The receipt returns over the same stream and the server records it.
    connection
        .submit_directive_receipt(receipt.clone())
        .await
        .expect("the server should record the returned receipt");

    // Now that a receipt exists, the directive is no longer pending: a
    // reconnect must not redeliver work that has already been answered.
    let mut connection = connect(&server, &node).await;
    connection
        .exchange_supervisor_frame(registration_frame(supervisor_id, &node.node_id))
        .await
        .expect("registration should replay");
    connection
        .exchange_supervisor_frame(session_frame(supervisor_id, session_id, "worker-1"))
        .await
        .expect("the session should replay");
    assert!(
        connection.next_directive().is_none(),
        "a directive that has been receipted must not be delivered again"
    );
}

/// ADR-0116 decision 3 and acceptance 4: redelivery is safe because the inbox
/// answers a repeated directive with its original receipt instead of acting
/// twice. Proven with a real disconnect and reconnect, and a real redelivery
/// from the server, rather than by calling `receive` twice in a loop.
#[tokio::test]
async fn a_redelivered_directive_replays_its_original_receipt() {
    let Some(database_url) = database_url() else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let node = enroll(&database_url).await;
    let server = start_sync_server(&database_url).await;
    let supervisor_id = "supervisor-slice3-replay";
    let session_id = "session-slice3-replay";

    let mut connection = connect(&server, &node).await;
    connection
        .exchange_supervisor_frame(registration_frame(supervisor_id, &node.node_id))
        .await
        .expect("registration should be accepted");
    connection
        .exchange_supervisor_frame(session_frame(supervisor_id, session_id, "worker-1"))
        .await
        .expect("the session should be accepted");

    let directive_pool = ackplane_server::db_pool::build_pool(
        &database_url,
        ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
    )
    .expect("the gated test database url builds a pool");
    let directives = DirectiveStore::connect(&directive_pool)
        .await
        .expect("the directive store should connect");
    directives
        .enqueue(pause_directive(
            &node,
            session_id,
            "directive-slice3-replay",
        ))
        .await
        .expect("the directive should be enqueued");

    let inbox = local_inbox(&node, supervisor_id, session_id);

    // First delivery, processed but the receipt never reaches the server --
    // the connection is dropped instead, which is the case a supervisor
    // cannot distinguish from a lost network.
    let mut first = connect(&server, &node).await;
    first
        .exchange_supervisor_frame(registration_frame(supervisor_id, &node.node_id))
        .await
        .expect("registration should replay");
    first
        .exchange_supervisor_frame(session_frame(supervisor_id, session_id, "worker-1"))
        .await
        .expect("the session should replay");
    let delivered = first
        .next_directive()
        .expect("the directive should be delivered");
    let original = inbox
        .receive(&delivered, OffsetDateTime::now_utc())
        .expect("the directive should be accepted");
    drop(first);

    // Reconnect. The server still has no receipt, so it redelivers.
    let mut second = connect(&server, &node).await;
    second
        .exchange_supervisor_frame(registration_frame(supervisor_id, &node.node_id))
        .await
        .expect("registration should replay");
    second
        .exchange_supervisor_frame(session_frame(supervisor_id, session_id, "worker-1"))
        .await
        .expect("the session should replay");
    let redelivered = second
        .next_directive()
        .expect("an unreceipted directive should be redelivered after a reconnect");
    assert_eq!(redelivered.directive_id, delivered.directive_id);

    let replayed = inbox
        .receive(&redelivered, OffsetDateTime::now_utc())
        .expect("a redelivered directive should replay rather than fail");
    assert_eq!(
        replayed, original,
        "redelivery must replay the original receipt, never act a second time"
    );

    second
        .submit_directive_receipt(replayed)
        .await
        .expect("the replayed receipt should be recorded");
}

/// ADR-0116 acceptance 3: a directive naming a capability this supervisor
/// never declared is refused locally, before any receipt claims otherwise. The
/// supervisor registered `Pause` only, so a `Prompt` directive addressed to it
/// must not be accepted just because the server was willing to carry it.
#[tokio::test]
async fn a_directive_for_an_undeclared_capability_is_refused_locally() {
    let Some(database_url) = database_url() else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let node = enroll(&database_url).await;
    let supervisor_id = "supervisor-slice3-capability";
    let session_id = "session-slice3-capability";
    let inbox = local_inbox(&node, supervisor_id, session_id);

    let mut directive = pause_directive(&node, session_id, "directive-slice3-capability");
    directive.kind = v1::DirectiveKind::Prompt as i32;
    directive.payload = Some(agent_directive::Payload::Prompt(v1::PromptDirective {
        instruction: "do something else".to_string(),
    }));
    directive.required_capability = "prompt.v1".to_string();
    directive.payload_digest =
        directive_payload_digest(&directive).expect("a prompt directive has a payload digest");

    let error = inbox
        .receive(&directive, OffsetDateTime::now_utc())
        .expect_err("a capability this supervisor never declared must be refused");
    let message = error.to_string();
    assert!(
        message.contains("capab"),
        "the refusal must name the capability problem: {message}"
    );
}

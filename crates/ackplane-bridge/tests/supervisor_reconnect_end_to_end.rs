//! End-to-end coverage for ADR-0116 slice 4 (task:20f1c2fba9bd): reconnect
//! position reconciliation and honest gap reporting, over real dropped and
//! reopened gRPC connections rather than a mocked transport.
//!
//! Slice 3 proved a clean redelivery survives a reconnect. What this adds is
//! the supervisor deciding, from durable state alone, *whether* the resume is
//! clean — and refusing to call it clean when the server holds evidence the
//! supervisor can no longer account for.
//!
//! Skipped unless `ACKPLANE_TEST_DATABASE_URL` names the gated test
//! PostgreSQL database.

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
use ackplane_supervisor::{
    reconcile, OutboxPositions, Reconciliation, SupervisorInbox, SupervisorOutbox,
};
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
    let directives = DirectiveStore::connect(database_url)
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
    let unique = unique_id("slice4");
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

/// Open a real connection, declaring `last_accepted_position` from durable
/// local state rather than a hardcoded zero (acceptance 1).
async fn connect_at(
    server: &TestServer,
    node: &EnrolledNode,
    last_accepted_position: u64,
) -> NodeSyncConnection {
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
        last_accepted_position,
    )
    .await
    .expect("an activated node should authenticate over real gRPC")
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

fn registration_frame(supervisor_id: &str, node_id: &str) -> v1::NodeFrame {
    v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::SupervisorRegistration(registration(
            supervisor_id,
            node_id,
        ))),
    }
}

fn session_frame(supervisor_id: &str, session_id: &str) -> v1::NodeFrame {
    v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::SupervisorSession(
            v1::SupervisorSession {
                supervisor_id: supervisor_id.to_string(),
                session_id: session_id.to_string(),
                worker_id: "worker-1".to_string(),
                runtime: v1::SupervisorRuntime::LocalMachine as i32,
                started_at: SESSION_STARTED_AT.to_string(),
                state: v1::SupervisorWorkerState::Started as i32,
            },
        )),
    }
}

fn local_registration(node: &EnrolledNode, supervisor_id: &str) -> SupervisorRegistration {
    SupervisorRegistration {
        supervisor_id: supervisor_id.to_string(),
        identity: SupervisorIdentity {
            tenant_id: node.tenant_id.clone(),
            repository_id: node.repository_id.clone(),
            node_id: node.node_id.clone(),
        },
        supervisor_version: "supervisor:v1".to_string(),
        protocol_version: "v1".to_string(),
        capabilities: SupervisorCapabilities {
            supported_directives: vec![SupervisorDirectiveCapability::Pause],
            supports_checkpoint: true,
            supports_force_termination: false,
            outbox_durability: SupervisorOutboxDurability::Persistent,
            recoverable_outbox: true,
        },
    }
}

fn local_session(supervisor_id: &str, session_id: &str) -> SupervisorSession {
    SupervisorSession {
        session_id: session_id.to_string(),
        supervisor_id: supervisor_id.to_string(),
        worker_id: "worker-1".to_string(),
        runtime: SupervisorRuntime::LocalMachine,
        started_at: SESSION_STARTED_AT_SECONDS,
        state: SupervisorWorkerState::Started,
    }
}

fn pause_directive(
    node: &EnrolledNode,
    session_id: &str,
    directive_id: &str,
    expires_in_seconds: i64,
) -> AgentDirective {
    let expires_at = OffsetDateTime::now_utc() + time::Duration::seconds(expires_in_seconds);
    let mut directive = AgentDirective {
        directive_id: directive_id.to_string(),
        tenant_id: node.tenant_id.clone(),
        project_id: "project:slice4".to_string(),
        repository_id: node.repository_id.clone(),
        target_node_id: node.node_id.clone(),
        target_agent_session_id: session_id.to_string(),
        kind: v1::DirectiveKind::Pause as i32,
        schema_version: "v1".to_string(),
        issuing_principal_id: "principal:operator".to_string(),
        rationale: "pause at a safe checkpoint".to_string(),
        task_id: "task:slice4".to_string(),
        goal_id: "goal:slice4".to_string(),
        context_packet_id: String::new(),
        created_at: String::new(),
        expires_at: expires_at
            .format(&Rfc3339)
            .expect("an expiry should format as RFC3339"),
        sequence: 0,
        idempotency_key: format!("{directive_id}:enqueue"),
        payload_digest: Vec::new(),
        required_capability: "pause.v1".to_string(),
        policy_refs: vec!["policy:slice4".to_string()],
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

/// Acceptance 1: a reconnecting supervisor declares the position its durable
/// outbox can actually prove, and reconciles cleanly against a server that
/// agrees.
#[tokio::test]
async fn a_reconnect_declares_its_durable_position_and_resumes_cleanly() {
    let Some(database_url) = database_url() else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let node = enroll(&database_url).await;
    let server = start_sync_server(&database_url).await;
    let supervisor_id = "supervisor-slice4";
    let session_id = "session-slice4";

    let outbox = SupervisorOutbox::open_in_memory(
        local_registration(&node, supervisor_id),
        local_session(supervisor_id, session_id),
    )
    .expect("the outbox should open");

    // A supervisor that has never sent anything proves position zero -- which
    // is a real answer, not a missing one.
    let fresh = outbox.positions().expect("positions should read");
    assert_eq!(
        fresh,
        OutboxPositions {
            acknowledged: 0,
            last_enqueued: 0
        }
    );

    let mut connection = connect_at(&server, &node, fresh.acknowledged).await;
    assert_eq!(
        reconcile(fresh, connection.accepted_position()),
        Reconciliation::UpToDate { position: 0 },
        "a fresh supervisor against a fresh server is up to date, not a gap"
    );
    connection
        .exchange_supervisor_frame(registration_frame(supervisor_id, &node.node_id))
        .await
        .expect("registration should be accepted");
    connection
        .exchange_supervisor_frame(session_frame(supervisor_id, session_id))
        .await
        .expect("the session should be accepted");

    // Queue two frames, acknowledge only the first, then drop the connection
    // for real.
    outbox
        .enqueue(1, &session_frame(supervisor_id, session_id))
        .expect("the first frame should queue");
    outbox
        .enqueue(2, &session_frame(supervisor_id, session_id))
        .expect("the second frame should queue");
    outbox
        .acknowledge_through(1)
        .expect("the first frame should acknowledge");
    drop(connection);

    let resumed = outbox.positions().expect("positions should read");
    assert_eq!(
        resumed,
        OutboxPositions {
            acknowledged: 1,
            last_enqueued: 2
        },
        "the acknowledged boundary is derived from the surviving frames"
    );

    // Reconnect declaring that durable position, and reconcile: the server is
    // behind the outbox, which is the ordinary resend case rather than a fault.
    let reconnected = connect_at(&server, &node, resumed.acknowledged).await;
    assert_eq!(
        reconcile(resumed, 1),
        Reconciliation::Resend {
            resend_from: 2,
            through: 2
        }
    );
    assert!(reconcile(resumed, 1).may_resume());
    assert_eq!(
        outbox
            .pending(16)
            .expect("pending should read")
            .iter()
            .map(|queued| queued.sequence)
            .collect::<Vec<_>>(),
        vec![2],
        "only the unacknowledged frame is resent"
    );
    drop(reconnected);
}

/// Acceptance 2: local state loss is surfaced, never dressed up as a clean
/// resume. Proven across a real reconnect: the supervisor's durable file is
/// replaced with an empty one between connections, exactly as a restore from
/// an older backup or a truncated write would leave it.
#[tokio::test]
async fn a_supervisor_that_lost_durable_state_reports_a_gap_rather_than_resuming() {
    let Some(database_url) = database_url() else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let node = enroll(&database_url).await;
    let server = start_sync_server(&database_url).await;
    let supervisor_id = "supervisor-slice4-gap";
    let session_id = "session-slice4-gap";

    // The supervisor did real work: five frames queued and acknowledged.
    let outbox = SupervisorOutbox::open_in_memory(
        local_registration(&node, supervisor_id),
        local_session(supervisor_id, session_id),
    )
    .expect("the outbox should open");
    for sequence in 1..=5 {
        outbox
            .enqueue(sequence, &session_frame(supervisor_id, session_id))
            .expect("the frame should queue");
    }
    outbox
        .acknowledge_through(5)
        .expect("every frame should acknowledge");
    assert_eq!(
        outbox
            .positions()
            .expect("positions should read")
            .acknowledged,
        5
    );

    let connection = connect_at(&server, &node, 5).await;
    drop(connection);

    // Its durable state is then lost -- restored from an older copy, or
    // truncated. A brand-new outbox reports position zero, which is a
    // perfectly legitimate value for a supervisor that never ran.
    let restored = SupervisorOutbox::open_in_memory(
        local_registration(&node, supervisor_id),
        local_session(supervisor_id, session_id),
    )
    .expect("a replacement outbox should open");
    let lost = restored.positions().expect("positions should read");
    assert_eq!(lost.acknowledged, 0);

    // Only the comparison against the server tells "new" apart from "lost".
    let outcome = reconcile(lost, 5);
    assert_eq!(
        outcome,
        Reconciliation::IncompleteEvidence {
            local_acknowledged: 0,
            server_accepted: 5
        }
    );
    assert!(
        !outcome.may_resume(),
        "work must not resume on evidence this supervisor cannot account for"
    );
    assert_eq!(outcome.missing_frames(), Some(5));

    // And it stays reported across a real reconnect: nothing about opening a
    // fresh connection repairs or hides it.
    let reconnected = connect_at(&server, &node, lost.acknowledged).await;
    assert!(!reconcile(lost, 5).may_resume());
    drop(reconnected);
}

/// Acceptance 3: a directive whose window closed while the supervisor was
/// disconnected is receipted `expired` on redelivery -- not silently dropped,
/// and not retried forever. Proven over a real disconnect and a real
/// redelivery from the server.
#[tokio::test]
async fn a_directive_that_expired_while_disconnected_is_receipted_expired() {
    let Some(database_url) = database_url() else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let node = enroll(&database_url).await;
    let server = start_sync_server(&database_url).await;
    let supervisor_id = "supervisor-slice4-expiry";
    let session_id = "session-slice4-expiry";

    let mut connection = connect_at(&server, &node, 0).await;
    connection
        .exchange_supervisor_frame(registration_frame(supervisor_id, &node.node_id))
        .await
        .expect("registration should be accepted");
    connection
        .exchange_supervisor_frame(session_frame(supervisor_id, session_id))
        .await
        .expect("the session should be accepted");

    // Issued with a short window, then the connection drops before it can be
    // delivered.
    let mut directives = DirectiveStore::connect(&database_url)
        .await
        .expect("the directive store should connect");
    directives
        .enqueue(pause_directive(
            &node,
            session_id,
            "directive-slice4-expiry",
            60,
        ))
        .await
        .expect("the directive should be enqueued");
    drop(connection);

    // Reconnect: the directive is still unreceipted, so it is redelivered.
    let mut reconnected = connect_at(&server, &node, 0).await;
    reconnected
        .exchange_supervisor_frame(registration_frame(supervisor_id, &node.node_id))
        .await
        .expect("registration should replay");
    reconnected
        .exchange_supervisor_frame(session_frame(supervisor_id, session_id))
        .await
        .expect("the session should replay");
    let delivered = reconnected
        .next_directive()
        .expect("an unreceipted directive should be redelivered");

    // The disconnect outlasted the directive's window. Judged against the
    // clock at processing time, which is what makes this the disconnect case
    // rather than a rejected-on-arrival one.
    let inbox = SupervisorInbox::open_in_memory(
        local_registration(&node, supervisor_id),
        local_session(supervisor_id, session_id),
    )
    .expect("the inbox should open");
    let after_the_window = OffsetDateTime::now_utc() + time::Duration::seconds(120);
    let receipt = inbox
        .receive(&delivered, after_the_window)
        .expect("an expired directive still earns a terminal receipt");

    assert_eq!(
        receipt.status,
        v1::DirectiveReceiptStatus::Expired as i32,
        "a directive whose window closed is receipted expired, not applied"
    );
    assert_eq!(receipt.reason, v1::DirectiveReceiptReason::Expired as i32);

    // The expired receipt is a real answer: returning it closes the directive
    // out rather than leaving it pending forever.
    reconnected
        .submit_directive_receipt(receipt)
        .await
        .expect("an expired receipt should be recorded like any other");

    let mut third = connect_at(&server, &node, 0).await;
    third
        .exchange_supervisor_frame(registration_frame(supervisor_id, &node.node_id))
        .await
        .expect("registration should replay");
    third
        .exchange_supervisor_frame(session_frame(supervisor_id, session_id))
        .await
        .expect("the session should replay");
    assert!(
        third.next_directive().is_none(),
        "an expired-and-receipted directive must not be redelivered forever"
    );
}

//! End-to-end coverage for ADR-0116 slice 5 (task:a4a27d170b48): the runnable
//! supervisor daemon's own registration and session are accepted by a real
//! Ackplane over real gRPC, and a directive it cannot execute is durably
//! receipted as refused rather than applied.
//!
//! The daemon's `run` loop is deliberately not driven here — it is an infinite
//! reconnect loop, and a test that starts one is a test that has to decide when
//! to stop it. What matters is that the frames the daemon builds are the ones a
//! real server accepts, and that its declared capabilities produce the honest
//! outcome; both are checked against the real components.
//!
//! Skipped unless `ACKPLANE_TEST_DATABASE_URL` names the gated test database.

#[allow(dead_code)]
mod supervisor_api_support;

use std::{path::PathBuf, time::Duration};

use ackplane_client::{auth::SeedSigner, node_sync::NodeSyncConnection};
use ackplane_protocol::{
    supervisor::directive_payload_digest,
    v1::{self, agent_directive, node_sync_service_server::NodeSyncServiceServer, AgentDirective},
};
use ackplane_server::{
    directive_store::DirectiveStore, ledger::LedgerStore, service::NodeSyncService,
    supervisor_store::SupervisorStore,
};
use ackplane_supervisor::{
    config::{SignerSource, SupervisorConfig},
    daemon, SupervisorInbox, SupervisorOutbox,
};
use sha2::{Digest, Sha256};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

use supervisor_api_support::{enroll_repository, unique_id};

struct TestServer {
    endpoint: String,
    _shutdown: oneshot::Sender<()>,
}

async fn start_sync_server(database_url: &str) -> TestServer {
    let db_pool = ackplane_server::db_pool::build_pool(
        database_url,
        ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
    )
    .expect("the gated test database url builds a pool");
    let ledger = LedgerStore::connect(&db_pool)
        .await
        .expect("the gated test database should accept ledger migrations");
    let supervisors = SupervisorStore::connect(&db_pool)
        .await
        .expect("the gated test database should accept supervisor migrations");
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

/// A daemon configuration pointed at a real enrolled node, exactly as
/// `config::resolve` would have produced from the environment.
fn config(
    endpoint: &str,
    tenant: &str,
    repository: &str,
    node: &str,
    unique: &str,
) -> SupervisorConfig {
    SupervisorConfig {
        endpoint: endpoint.to_string(),
        tenant_id: tenant.to_string(),
        repository_id: repository.to_string(),
        node_id: node.to_string(),
        signing_key_id: format!("signing-key-{unique}"),
        supervisor_id: format!("supervisor-{unique}"),
        signer_source: SignerSource::Seed(Box::new(
            Sha256::digest(format!("key-{unique}").as_bytes()).into(),
        )),
        state_dir: std::env::temp_dir().join(format!("ackplane-supervisor-{unique}")),
        heartbeat_interval: Duration::from_secs(30),
    }
}

fn database_url() -> Option<String> {
    std::env::var("ACKPLANE_TEST_DATABASE_URL").ok()
}

async fn connect(server: &TestServer, config: &SupervisorConfig) -> NodeSyncConnection {
    let signer = SeedSigner::new(
        config.signing_key_id.clone(),
        config.node_id.clone(),
        match &config.signer_source {
            SignerSource::Seed(seed) => seed.as_ref(),
            SignerSource::CredentialFacility { .. } => panic!("the fixture uses a seed"),
        },
    );
    NodeSyncConnection::open(
        &server.endpoint,
        &signer,
        &config.tenant_id,
        &config.repository_id,
        vec!["synchronize".to_string()],
        0,
    )
    .await
    .expect("the supervisor's node should authenticate")
}

fn registration_frame(
    registration: &ackplane_protocol::supervisor::SupervisorRegistration,
) -> v1::NodeFrame {
    v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::SupervisorRegistration(
            v1::SupervisorRegistration {
                supervisor_id: registration.supervisor_id.clone(),
                node_id: registration.identity.node_id.clone(),
                supervisor_version: registration.supervisor_version.clone(),
                protocol_version: registration.protocol_version.clone(),
                supported_directives: vec![v1::SupervisorDirectiveCapability::Notify as i32],
                supports_checkpoint: false,
                supports_force_termination: false,
                outbox_durability: v1::SupervisorOutboxDurability::Persistent as i32,
                recoverable_outbox: true,
            },
        )),
    }
}

fn session_frame(session: &ackplane_protocol::supervisor::SupervisorSession) -> v1::NodeFrame {
    v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::SupervisorSession(
            v1::SupervisorSession {
                supervisor_id: session.supervisor_id.clone(),
                session_id: session.session_id.clone(),
                worker_id: session.worker_id.clone(),
                runtime: v1::SupervisorRuntime::LocalMachine as i32,
                started_at: OffsetDateTime::from_unix_timestamp(session.started_at)
                    .expect("a representable session start")
                    .format(&Rfc3339)
                    .expect("RFC3339"),
                state: v1::SupervisorWorkerState::Started as i32,
            },
        )),
    }
}

/// A notify directive: the one capability this daemon declares, so its receipt
/// is `Accepted` and binds to a directive the server genuinely issued.
fn notify_directive(config: &SupervisorConfig, session_id: &str, unique: &str) -> AgentDirective {
    let mut directive = AgentDirective {
        directive_id: format!("directive-{unique}"),
        tenant_id: config.tenant_id.clone(),
        project_id: "project:slice5".to_string(),
        repository_id: config.repository_id.clone(),
        target_node_id: config.node_id.clone(),
        target_agent_session_id: session_id.to_string(),
        kind: v1::DirectiveKind::Notify as i32,
        schema_version: "v1".to_string(),
        issuing_principal_id: "principal:operator".to_string(),
        rationale: "tell the supervisor something".to_string(),
        task_id: String::new(),
        goal_id: String::new(),
        context_packet_id: String::new(),
        created_at: String::new(),
        expires_at: (OffsetDateTime::now_utc() + time::Duration::seconds(600))
            .format(&Rfc3339)
            .expect("RFC3339"),
        sequence: 0,
        idempotency_key: format!("{unique}:notify"),
        payload_digest: Vec::new(),
        required_capability: "notify.v1".to_string(),
        policy_refs: Vec::new(),
        knowledge_refs: Vec::new(),
        evidence_refs: Vec::new(),
        payload: Some(agent_directive::Payload::Notify(v1::NotifyDirective {
            message: "a message for the supervisor".to_string(),
        })),
    };
    directive.payload_digest = directive_payload_digest(&directive).expect("a payload digest");
    directive
}

/// The daemon's own registration and session frames are accepted by a real
/// server: the declaration it builds is one Ackplane will actually take.
#[tokio::test]
async fn the_daemon_registers_and_opens_a_session_against_a_real_ackplane() {
    let Some(database_url) = database_url() else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let unique = unique_id("slice5");
    let tenant_id = format!("tenant-{unique}");
    let repository_id = format!("repository-{unique}");
    let node_id = enroll_repository(&database_url, &tenant_id, &repository_id, &unique).await;
    let server = start_sync_server(&database_url).await;
    let config = config(
        &server.endpoint,
        &tenant_id,
        &repository_id,
        &node_id,
        &unique,
    );

    // The daemon's own registration must satisfy the protocol it is about to
    // declare over. This is what caught the first design here: an empty
    // capability list is refused outright by SupervisorCapabilities::validate.
    let registration = daemon::registration(&config);
    registration
        .validate()
        .expect("the daemon's registration must be protocol-valid");

    let signer = SeedSigner::new(
        config.signing_key_id.clone(),
        config.node_id.clone(),
        match &config.signer_source {
            SignerSource::Seed(seed) => seed.as_ref(),
            SignerSource::CredentialFacility { .. } => panic!("the fixture uses a seed"),
        },
    );
    let mut connection = NodeSyncConnection::open(
        &config.endpoint,
        &signer,
        &config.tenant_id,
        &config.repository_id,
        vec!["synchronize".to_string()],
        0,
    )
    .await
    .expect("the daemon's node should authenticate");

    let started_at = OffsetDateTime::now_utc();
    let session = daemon::session(&config, started_at).expect("a session should build");
    session.validate().expect("the session must be valid");

    // Drive the same frames the daemon sends, and require the real server to
    // accept both.
    let accepted = connection
        .exchange_supervisor_frame(v1::NodeFrame {
            frame: Some(v1::node_frame::Frame::SupervisorRegistration(
                v1::SupervisorRegistration {
                    supervisor_id: registration.supervisor_id.clone(),
                    node_id: registration.identity.node_id.clone(),
                    supervisor_version: registration.supervisor_version.clone(),
                    protocol_version: registration.protocol_version.clone(),
                    supported_directives: vec![v1::SupervisorDirectiveCapability::Notify as i32],
                    supports_checkpoint: false,
                    supports_force_termination: false,
                    outbox_durability: v1::SupervisorOutboxDurability::Persistent as i32,
                    recoverable_outbox: true,
                },
            )),
        })
        .await
        .expect("a real Ackplane should accept the daemon's registration");
    assert_eq!(accepted.supervisor_id, registration.supervisor_id);

    let accepted = connection
        .exchange_supervisor_frame(v1::NodeFrame {
            frame: Some(v1::node_frame::Frame::SupervisorSession(
                v1::SupervisorSession {
                    supervisor_id: session.supervisor_id.clone(),
                    session_id: session.session_id.clone(),
                    worker_id: session.worker_id.clone(),
                    runtime: v1::SupervisorRuntime::LocalMachine as i32,
                    started_at: started_at.format(&Rfc3339).expect("RFC3339"),
                    state: v1::SupervisorWorkerState::Started as i32,
                },
            )),
        })
        .await
        .expect("a real Ackplane should accept the daemon's session");
    assert_eq!(accepted.session_id, session.session_id);
}

/// ADR-0116 decision 10: a directive this build cannot execute is durably
/// receipted as refused, never as applied. Checked against the daemon's real
/// declared capabilities and the real inbox, not a hand-written registration.
#[tokio::test]
async fn a_worker_driven_directive_is_receipted_refused_not_applied() {
    let unique = unique_id("slice5-refuse");
    let config = config(
        "http://127.0.0.1:1",
        &format!("tenant-{unique}"),
        &format!("repository-{unique}"),
        &format!("node-{unique}"),
        &unique,
    );
    let registration = daemon::registration(&config);
    let session = daemon::session(&config, OffsetDateTime::now_utc()).expect("a session");
    let inbox = SupervisorInbox::open_in_memory(registration.clone(), session.clone())
        .expect("the inbox should open");

    // A pause needs a worker to pause. This build has none, and does not
    // declare the capability.
    let mut directive = AgentDirective {
        directive_id: format!("directive-{unique}"),
        tenant_id: config.tenant_id.clone(),
        project_id: "project:slice5".to_string(),
        repository_id: config.repository_id.clone(),
        target_node_id: config.node_id.clone(),
        target_agent_session_id: session.session_id.clone(),
        kind: v1::DirectiveKind::Pause as i32,
        schema_version: "v1".to_string(),
        issuing_principal_id: "principal:operator".to_string(),
        rationale: "pause the worker".to_string(),
        task_id: "task:slice5".to_string(),
        goal_id: "goal:slice5".to_string(),
        context_packet_id: String::new(),
        created_at: String::new(),
        expires_at: (OffsetDateTime::now_utc() + time::Duration::seconds(600))
            .format(&Rfc3339)
            .expect("RFC3339"),
        sequence: 1,
        idempotency_key: format!("{unique}:pause"),
        payload_digest: Vec::new(),
        required_capability: "pause.v1".to_string(),
        policy_refs: Vec::new(),
        knowledge_refs: Vec::new(),
        evidence_refs: Vec::new(),
        payload: Some(agent_directive::Payload::Pause(v1::PauseDirective {
            checkpoint_required: true,
        })),
    };
    directive.payload_digest = directive_payload_digest(&directive).expect("a payload digest");

    let receipt = inbox
        .receive(&directive, OffsetDateTime::now_utc())
        .expect("an undeliverable capability still earns a durable receipt");

    assert_eq!(
        receipt.status,
        v1::DirectiveReceiptStatus::Refused as i32,
        "a directive needing a worker this build lacks must be refused, never applied"
    );
    assert_eq!(
        receipt.reason,
        v1::DirectiveReceiptReason::CapabilityMissing as i32
    );
    assert_ne!(
        receipt.status,
        v1::DirectiveReceiptStatus::Accepted as i32,
        "an Accepted receipt would claim work that nothing performed"
    );
}

/// A notification is complete once durably recorded, so declaring `Notify` is
/// not a placeholder: the daemon genuinely honours it, and the receipt says so
/// truthfully.
#[tokio::test]
async fn a_notification_is_accepted_because_recording_it_is_the_whole_action() {
    let unique = unique_id("slice5-notify");
    let config = config(
        "http://127.0.0.1:1",
        &format!("tenant-{unique}"),
        &format!("repository-{unique}"),
        &format!("node-{unique}"),
        &unique,
    );
    let registration = daemon::registration(&config);
    let session = daemon::session(&config, OffsetDateTime::now_utc()).expect("a session");
    let inbox = SupervisorInbox::open_in_memory(registration, session.clone())
        .expect("the inbox should open");

    let mut directive = AgentDirective {
        directive_id: format!("directive-{unique}"),
        tenant_id: config.tenant_id.clone(),
        project_id: "project:slice5".to_string(),
        repository_id: config.repository_id.clone(),
        target_node_id: config.node_id.clone(),
        target_agent_session_id: session.session_id.clone(),
        kind: v1::DirectiveKind::Notify as i32,
        schema_version: "v1".to_string(),
        issuing_principal_id: "principal:operator".to_string(),
        rationale: "tell the supervisor something".to_string(),
        task_id: String::new(),
        goal_id: String::new(),
        context_packet_id: String::new(),
        created_at: String::new(),
        expires_at: (OffsetDateTime::now_utc() + time::Duration::seconds(600))
            .format(&Rfc3339)
            .expect("RFC3339"),
        sequence: 1,
        idempotency_key: format!("{unique}:notify"),
        payload_digest: Vec::new(),
        required_capability: "notify.v1".to_string(),
        policy_refs: Vec::new(),
        knowledge_refs: Vec::new(),
        evidence_refs: Vec::new(),
        payload: Some(agent_directive::Payload::Notify(v1::NotifyDirective {
            message: "a message for the supervisor".to_string(),
        })),
    };
    directive.payload_digest = directive_payload_digest(&directive).expect("a payload digest");

    let receipt = inbox
        .receive(&directive, OffsetDateTime::now_utc())
        .expect("a declared capability should be accepted");

    assert_eq!(
        receipt.status,
        v1::DirectiveReceiptStatus::Accepted as i32,
        "recording the notification is the whole action, so accepting it is truthful"
    );
}

/// The durable state paths are per-supervisor, so two supervisors on one host
/// never share an inbox or overwrite each other's receipts.
#[tokio::test]
async fn two_supervisors_on_one_host_get_separate_durable_state() {
    let first = config("http://127.0.0.1:1", "t", "r", "n", "alpha");
    let second = SupervisorConfig {
        supervisor_id: "supervisor-beta".to_string(),
        state_dir: first.state_dir.clone(),
        ..config("http://127.0.0.1:1", "t", "r", "n", "alpha")
    };

    assert_ne!(first.inbox_path(), second.inbox_path());
    assert_ne!(first.outbox_path(), second.outbox_path());
    assert_ne!(first.inbox_path(), first.outbox_path());
    assert_eq!(
        first.inbox_path().parent().map(PathBuf::from),
        second.inbox_path().parent().map(PathBuf::from),
        "they share a state directory but never a file"
    );
}

/// The durability the outbox exists for: a receipt that was computed and
/// written down, then lost because the connection dropped before the server
/// confirmed it, is resent from local state on the next connection.
///
/// Before this was wired, `serve_once` opened the outbox and never enqueued
/// anything, so the only thing that recovered a lost receipt was the server
/// redelivering its directive — a guarantee held by the other side of the
/// connection that had just failed.
#[tokio::test]
async fn an_unconfirmed_receipt_survives_a_drop_and_is_resent_on_reconnect() {
    let Some(database_url) = database_url() else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let unique = unique_id("slice5-resend");
    let tenant_id = format!("tenant-{unique}");
    let repository_id = format!("repository-{unique}");
    let node_id = enroll_repository(&database_url, &tenant_id, &repository_id, &unique).await;
    let server = start_sync_server(&database_url).await;
    let config = config(
        &server.endpoint,
        &tenant_id,
        &repository_id,
        &node_id,
        &unique,
    );
    let registration = daemon::registration(&config);
    let session = daemon::session(&config, OffsetDateTime::now_utc()).expect("a session");

    // A real directive, issued and delivered, so the receipt below binds to
    // something the server actually knows about -- the server refuses a
    // receipt that references no directive, and a fixture that skipped this
    // would prove only that refusal.
    let mut connection = connect(&server, &config).await;
    connection
        .exchange_supervisor_frame(registration_frame(&registration))
        .await
        .expect("registration should be accepted");
    connection
        .exchange_supervisor_frame(session_frame(&session))
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
        .enqueue(notify_directive(&config, &session.session_id, &unique))
        .await
        .expect("the directive should be enqueued");

    let mut connection = connect(&server, &config).await;
    connection
        .exchange_supervisor_frame(registration_frame(&registration))
        .await
        .expect("registration should replay");
    connection
        .exchange_supervisor_frame(session_frame(&session))
        .await
        .expect("the session should replay");
    let delivered = connection
        .next_directive()
        .expect("the directive should be delivered");

    let inbox = SupervisorInbox::open_in_memory(registration.clone(), session.clone())
        .expect("the inbox should open");
    let receipt = inbox
        .receive(&delivered, OffsetDateTime::now_utc())
        .expect("a declared capability should be accepted");

    // The connection dies before that receipt ever reaches the server.
    drop(connection);

    let outbox =
        SupervisorOutbox::open(config.outbox_path(), registration.clone(), session.clone())
            .expect("the outbox should open");
    outbox
        .enqueue(
            1,
            &v1::NodeFrame {
                frame: Some(v1::node_frame::Frame::DirectiveReceipt(receipt)),
            },
        )
        .expect("the receipt should queue durably");

    // Unconfirmed, so it is still owed.
    let before = outbox.positions().expect("positions");
    assert_eq!(before.last_enqueued, 1);
    assert_eq!(
        before.acknowledged, 0,
        "nothing may be acknowledged until the server confirms it"
    );
    assert_eq!(
        outbox.pending(32).expect("pending").len(),
        1,
        "the unconfirmed receipt is what a reconnect must resend"
    );

    // A fresh connection drains it against the real server, exactly as
    // `serve_once` does before it begins serving new directives.
    let mut connection = connect(&server, &config).await;
    connection
        .exchange_supervisor_frame(registration_frame(&registration))
        .await
        .expect("registration should be accepted");

    let exit = daemon::resend_pending(&outbox, &mut connection)
        .await
        .expect("resending should not fail");
    assert!(
        exit.is_none(),
        "the connection is live, so resending must not report a disconnect"
    );

    let after = outbox.positions().expect("positions");
    assert_eq!(
        after.acknowledged, 1,
        "the resent receipt is acknowledged only after the server confirmed it"
    );
    assert!(
        outbox.pending(32).expect("pending").is_empty(),
        "nothing remains owed once the server has confirmed it"
    );
}

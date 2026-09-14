use std::time::{Duration, SystemTime};

use ackplane_client::companion::NodeClient;
use ackplane_node::SigningBinding;
use ackplane_protocol::{enrollment::public_key_fingerprint, v1};
use ackplane_server::{
    db_pool::{build_pool, TEST_POOL_MAX_SIZE},
    enrollment_store::EnrollmentStore,
    ledger::LedgerStore,
    service::NodeSyncService,
    signing_keys::{self, SigningKeyRecord},
    supervisor_store::SupervisorStore,
};
use ackplane_supervisor::{config::SupervisorConfig, daemon, SupervisorOutbox};
use time::OffsetDateTime;
use tokio_stream::wrappers::TcpListenerStream;

#[path = "../../ackplane-node/tests/support/companion.rs"]
mod companion;

// Restart used a new clock value under the persisted session ID, which the real
// server rejected forever. Both runs must restore the same fact and keep heartbeating.
#[tokio::test]
async fn restarted_daemon_preserves_its_session_and_reports_a_new_server_heartbeat() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let root = tempfile::tempdir().unwrap();
    let pool = build_pool(&database_url, TEST_POOL_MAX_SIZE).unwrap();
    EnrollmentStore::connect(&pool).await.unwrap();
    let ledger = LedgerStore::connect(&pool).await.unwrap();
    let supervisors = SupervisorStore::connect(&pool).await.unwrap();
    let mut seed = [0_u8; 32];
    getrandom::getrandom(&mut seed).unwrap();
    let suffix: String = seed[..8].iter().map(|byte| format!("{byte:02x}")).collect();
    let binding = SigningBinding {
        tenant_id: format!("restart:{suffix}"),
        repository_id: "repository:restart".into(),
        node_id: "node:restart".into(),
        key_id: format!("key:{suffix}"),
    };
    let config = SupervisorConfig {
        node: NodeClient::new(
            root.path().join("node"),
            binding.tenant_id.clone(),
            binding.repository_id.clone(),
        ),
        supervisor_id: "restart-session".into(),
        state_dir: root.path().join("queue"),
        heartbeat_interval: Duration::from_millis(100),
        workers: Default::default(),
    };
    let public_key = ed25519_dalek::SigningKey::from_bytes(&seed)
        .verifying_key()
        .to_bytes()
        .to_vec();
    {
        let mut connection = pool.get().await.unwrap();
        let transaction = connection.transaction().await.unwrap();
        signing_keys::register(
            &transaction,
            &SigningKeyRecord {
                signing_key_id: binding.key_id.clone(),
                tenant_id: binding.tenant_id.clone(),
                repository_id: binding.repository_id.clone(),
                node_id: binding.node_id.clone(),
                public_key_fingerprint: public_key_fingerprint(&public_key),
                public_key,
                activated_at: SystemTime::now() - Duration::from_secs(1),
                expires_at: None,
            },
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let service = NodeSyncService::with_supervisor_store(
        ledger,
        SupervisorStore::connect(&pool).await.unwrap(),
        v1::FlowControl {
            max_in_flight_batches: 16,
            max_batch_bytes: 1_048_576,
        },
    );
    let (shutdown, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(v1::node_sync_service_server::NodeSyncServiceServer::new(
                service,
            ))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let registration = daemon::registration(&config, &binding.node_id);
    let original = daemon::session(
        &config,
        OffsetDateTime::now_utc() - time::Duration::seconds(60),
    )
    .unwrap();
    drop(
        SupervisorOutbox::open(config.outbox_path(), registration.clone(), original.clone())
            .unwrap(),
    );
    let mut last_heartbeat = 0;
    for _run in 0..2 {
        let companion = companion::TestCompanion::start(
            &endpoint,
            binding.clone(),
            &seed,
            &config.node.state_dir,
        )
        .await;
        let (stop, stopping) = tokio::sync::watch::channel(false);
        let observe = async {
            let mut interval = tokio::time::interval(Duration::from_millis(25));
            loop {
                interval.tick().await;
                let entries = supervisors
                    .list_supervisors(&binding.tenant_id, &binding.repository_id)
                    .await
                    .unwrap();
                let Some(heartbeat) = entries.first().and_then(|entry| entry.last_heartbeat_at)
                else {
                    continue;
                };
                if heartbeat <= last_heartbeat {
                    continue;
                }
                assert_eq!(entries.len(), 1);
                let sessions = supervisors
                    .list_sessions(
                        &binding.tenant_id,
                        &binding.repository_id,
                        &config.supervisor_id,
                    )
                    .await
                    .unwrap();
                assert_eq!(sessions.len(), 1);
                assert_eq!(sessions[0].session, original);
                stop.send_replace(true);
                return Ok::<_, daemon::DaemonError>(heartbeat);
            }
        };
        let (_, heartbeat) = tokio::time::timeout(Duration::from_secs(15), async {
            tokio::try_join!(
                daemon::run(&config, Duration::from_millis(10), stopping),
                observe
            )
        })
        .await
        .expect("the restarted daemon must heartbeat and stop within the deadline")
        .expect("both daemon runs must exit successfully");
        last_heartbeat = heartbeat;
        drop(companion);
        let outbox = SupervisorOutbox::open_read_only(
            config.outbox_path(),
            registration.clone(),
            original.clone(),
        )
        .unwrap();
        assert_eq!(outbox.session(), &original);
        assert!(outbox.pending(10).unwrap().is_empty());
    }
    let _ = shutdown.send(());
    tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .unwrap()
        .unwrap();
}

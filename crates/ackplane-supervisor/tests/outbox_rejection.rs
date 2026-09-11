use std::time::{Duration, SystemTime};

use ackplane_client::{companion::NodeClient, NodeSyncConnection, SeedSigner};
use ackplane_protocol::{
    enrollment::public_key_fingerprint, supervisor::SupervisorWorkerState, v1,
};
use ackplane_server::{
    db_pool::{build_pool, TEST_POOL_MAX_SIZE},
    enrollment_store::EnrollmentStore,
    ledger::LedgerStore,
    service::NodeSyncService,
    signing_keys::{self, SigningKeyRecord},
    supervisor_store::SupervisorStore,
};
use ackplane_supervisor::{
    config::SupervisorConfig, daemon, OutboxError, OutboxPositions, SupervisorOutbox,
};
use prost::Message;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio_stream::wrappers::TcpListenerStream;

// Permanent rejection used to prune the refused frame and advance acknowledgement,
// letting later receipts hide the missing evidence. Preserve it and stop instead.
#[tokio::test]
async fn permanent_rejection_preserves_the_unaccepted_outbox_tail_after_reopen() {
    exercise_rejection(Rejection::Permanent).await;
}

#[tokio::test]
async fn retryable_rejection_preserves_the_queue_and_requests_reconnect() {
    exercise_rejection(Rejection::Retryable).await;
}

// Unknown fields used to disappear before transport, changing durable evidence.
// Refuse the entire loaded batch before any frame is sent or acknowledged.
#[tokio::test]
async fn unreplayable_wire_bytes_stop_the_sender_without_sending_or_pruning_any_frame() {
    exercise_rejection(Rejection::UnsupportedEncoding).await;
}

enum Rejection {
    Permanent,
    Retryable,
    UnsupportedEncoding,
}

async fn exercise_rejection(rejection: Rejection) {
    let permanent = !matches!(rejection, Rejection::Retryable);
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let node_id = "node:outbox-rejection";
    let key_id = format!("key:{suffix}");
    let config = SupervisorConfig {
        node: NodeClient::new(
            root.path().into(),
            format!("outbox-rejection:{suffix}"),
            "repository:outbox-rejection".into(),
        ),
        supervisor_id: "outbox-rejection".into(),
        state_dir: root.path().to_path_buf(),
        heartbeat_interval: Duration::from_secs(1),
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
                signing_key_id: key_id.clone(),
                tenant_id: config.node.tenant_id.clone(),
                repository_id: config.node.repository_id.clone(),
                node_id: node_id.into(),
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
    let flow_control = v1::FlowControl {
        max_in_flight_batches: 16,
        max_batch_bytes: 1_048_576,
    };
    let service = if permanent {
        NodeSyncService::with_supervisor_store(
            ledger,
            SupervisorStore::connect(&pool).await.unwrap(),
            flow_control,
        )
    } else {
        NodeSyncService::new(ledger, flow_control)
    };
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
    let signer = SeedSigner::new(key_id, node_id, &seed);
    let mut connection = tokio::time::timeout(
        Duration::from_secs(10),
        NodeSyncConnection::open(
            &endpoint,
            &signer,
            &config.node.tenant_id,
            &config.node.repository_id,
            vec!["synchronize".into()],
            0,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    let registration = daemon::registration(&config, node_id);
    let started_at = OffsetDateTime::now_utc();
    let session = daemon::session(&config, started_at).unwrap();
    let wire_registration = v1::NodeFrame {
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
    };
    if permanent {
        connection
            .exchange_supervisor_frame(wire_registration.clone())
            .await
            .unwrap();
        connection
            .exchange_supervisor_frame(v1::NodeFrame {
                frame: Some(v1::node_frame::Frame::SupervisorSession(
                    v1::SupervisorSession {
                        supervisor_id: session.supervisor_id.clone(),
                        session_id: session.session_id.clone(),
                        worker_id: session.worker_id.clone(),
                        runtime: v1::SupervisorRuntime::LocalMachine as i32,
                        started_at: started_at.format(&Rfc3339).unwrap(),
                        state: v1::SupervisorWorkerState::Started as i32,
                    },
                )),
            })
            .await
            .unwrap();
    }
    let path = config.outbox_path();
    let outbox = SupervisorOutbox::open(&path, registration.clone(), session.clone()).unwrap();
    let receipt = |state, key: &str| v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::SupervisorLifecycleReceipt(
            v1::SupervisorLifecycleReceipt {
                supervisor_id: session.supervisor_id.clone(),
                session_id: session.session_id.clone(),
                worker_id: session.worker_id.clone(),
                occurred_at: OffsetDateTime::now_utc().format(&Rfc3339).unwrap(),
                state,
                reason: v1::SupervisorLifecycleReason::Unspecified as i32,
                idempotency_key: key.into(),
                outbox_sequence: None,
            },
        )),
    };
    let first = outbox
        .enqueue_next(receipt(
            v1::SupervisorWorkerState::Started as i32,
            "started",
        ))
        .unwrap();
    let rejected = outbox
        .enqueue_next(receipt(i32::MAX, "invalid-state"))
        .unwrap();
    let tail = outbox
        .enqueue_next(receipt(
            v1::SupervisorWorkerState::Terminated as i32,
            "terminated",
        ))
        .unwrap();
    let mut stored = outbox
        .pending(10)
        .unwrap()
        .into_iter()
        .map(|queued| (queued.sequence, queued.frame.encode_to_vec()))
        .collect::<Vec<_>>();
    if matches!(rejection, Rejection::UnsupportedEncoding) {
        stored[0].1.extend_from_slice(&[0x98, 0x06, 0x01]);
        assert_eq!(
            v1::NodeFrame::decode(stored[0].1.as_slice()).unwrap(),
            first.frame
        );
        rusqlite::Connection::open(&path)
            .unwrap()
            .execute(
                "UPDATE outbound_frames SET frame = ?1 WHERE sequence = 1",
                [&stored[0].1],
            )
            .unwrap();
    }
    let expected = if permanent {
        vec![rejected, tail]
    } else {
        vec![first, rejected, tail]
    };
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        daemon::resend_pending(&outbox, &mut connection),
    )
    .await
    .unwrap();
    let server_position = if permanent {
        connection
            .exchange_supervisor_frame(wire_registration)
            .await
            .unwrap()
            .accepted_outbox_sequence
    } else {
        None
    };
    let history = supervisors
        .lifecycle_history(
            &config.node.tenant_id,
            &config.node.repository_id,
            &session.session_id,
        )
        .await
        .unwrap();
    drop(connection);
    let _ = shutdown.send(());
    tokio::time::timeout(Duration::from_secs(10), server)
        .await
        .unwrap()
        .unwrap();
    drop(outbox);
    let reopened = SupervisorOutbox::open(&path, registration, session).unwrap();

    if matches!(rejection, Rejection::UnsupportedEncoding) {
        assert!(matches!(
            result,
            Err(daemon::DaemonError::Outbox(
                OutboxError::UnsupportedStoredEncoding { sequence: 1 }
            ))
        ));
        assert_eq!(server_position, None, "no sequenced receipt was accepted");
        assert!(history.is_empty());
        assert_eq!(
            reopened.positions().unwrap(),
            OutboxPositions {
                acknowledged: 0,
                last_enqueued: 3
            }
        );
        assert!(matches!(
            reopened.pending(10),
            Err(OutboxError::UnsupportedStoredEncoding { sequence: 1 })
        ));
        assert!(reopened
            .acknowledged_lifecycle_receipts(0, 10)
            .unwrap()
            .is_empty());
        let database = rusqlite::Connection::open(&path).unwrap();
        let retained: Vec<(u64, Vec<u8>)> = database
            .prepare("SELECT sequence, frame FROM outbound_frames ORDER BY sequence")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(retained, stored);
        return;
    }

    assert_eq!(
        reopened.positions().unwrap(),
        OutboxPositions {
            acknowledged: u64::from(permanent),
            last_enqueued: 3
        },
        "a server refusal must not advance the locally accepted boundary"
    );
    let pending = reopened.pending(10).unwrap();
    assert_eq!(pending, expected);
    assert_eq!(
        pending
            .iter()
            .map(|queued| queued.frame.encode_to_vec())
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|queued| queued.frame.encode_to_vec())
            .collect::<Vec<_>>()
    );
    if permanent {
        assert_eq!(
            server_position,
            Some(1),
            "the rejected frame must stop delivery of later frames"
        );
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].receipt.state, SupervisorWorkerState::Started);
        let error = result.expect_err("permanent rejection must stop the daemon");
        assert!(error
            .to_string()
            .contains("queued evidence retained for operator recovery"));
        let daemon::DaemonError::RejectedFrame {
            sequence,
            reason,
            diagnostic,
        } = error
        else {
            panic!("expected typed permanent rejection, got {error:?}");
        };
        assert_eq!(sequence, 2);
        assert_ne!(reason as i32, 0);
        assert!(diagnostic.contains("worker state"), "{diagnostic}");
    } else {
        assert!(history.is_empty());
        assert!(matches!(result, Ok(Some(daemon::DaemonExit::Disconnected))));
    }
}

use std::{
    collections::BTreeMap,
    fs,
    future::Future,
    process::{Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, SystemTime},
};

use ackplane_protocol::{
    context_packet::ContextPacketUseStatus, supervisor::SupervisorWorkerState, v1,
};
use ackplane_server::{
    claim_service::ClaimDelegationService,
    claim_store::ClaimStore,
    constitution_store::{ClauseSnapshot, ConstitutionStore, PublishConstitutionRequest},
    context_packet_store::ContextPacketStore,
    context_service::ContextService,
    db_pool::{build_pool, PgPool, TEST_POOL_MAX_SIZE},
    directive_store::DirectiveStore,
    knowledge_store::{KnowledgeStore, RecordKnowledgeRequest},
    ledger::LedgerStore,
    service::NodeSyncService,
    signing_keys::{self, SigningKeyRecord},
    supervisor_store::SupervisorStore,
    work_command_store::*,
    work_query_service::WorkQueryService,
    work_store::{NewWorkTask, WorkStore, WorkTaskState},
};
use ackplane_supervisor::WorkerCommand;
use command_group::{CommandGroup, GroupChild};
use tokio_stream::{
    wrappers::{ReceiverStream, TcpListenerStream},
    StreamExt,
};

struct Daemon(GroupChild);

#[derive(Default)]
struct Scenario {
    stop_while_active: bool,
    stop_before_spawn: bool,
    release_failures: usize,
    fail_first_slot: bool,
}

#[derive(Clone)]
struct ReleaseFailures(Arc<AtomicUsize>);

impl tonic::service::Interceptor for ReleaseFailures {
    fn call(&mut self, request: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        if self
            .0
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                remaining.checked_sub(1)
            })
            .is_ok()
        {
            Err(tonic::Status::unavailable(
                "injected lease release transport failure",
            ))
        } else {
            Ok(request)
        }
    }
}

impl Daemon {
    async fn shutdown(&mut self) {
        #[cfg(unix)]
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(self.0.id() as i32),
            nix::sys::signal::Signal::SIGTERM,
        )
        .unwrap();
        let exit = self.wait().await;
        assert!(
            exit.success(),
            "supervisor did not shut down cleanly: {exit}"
        );
    }

    async fn wait(&mut self) -> ExitStatus {
        wait_for("supervisor exit", || {
            let exit = self.0.try_wait().unwrap();
            async move { exit }
        })
        .await
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct ContextReplyGate {
    service: NodeSyncService,
    held_contexts: Option<Arc<tokio::sync::Semaphore>>,
    reject_first_use: Option<Arc<tokio::sync::Semaphore>>,
}

#[tonic::async_trait]
impl v1::node_sync_service_server::NodeSyncService for ContextReplyGate {
    type SynchronizeStream = ReceiverStream<Result<v1::AckplaneFrame, tonic::Status>>;

    async fn synchronize(
        &self,
        request: tonic::Request<tonic::Streaming<v1::NodeFrame>>,
    ) -> Result<tonic::Response<Self::SynchronizeStream>, tonic::Status> {
        let mut stream = self.service.synchronize(request).await?.into_inner();
        let (sender, receiver) = tokio::sync::mpsc::channel(16);
        let held_contexts = self.held_contexts.clone();
        let reject_first_use = self.reject_first_use.clone();
        tokio::spawn(async move {
            let mut first_slot = false;
            while let Some(frame) = stream.next().await {
                if let Ok(reply) = &frame {
                    if let Some(v1::ackplane_frame::Frame::SupervisorFrameReceipt(receipt)) =
                        reply.frame.as_ref()
                    {
                        first_slot = receipt.supervisor_id.starts_with("multi-agent-first-");
                    }
                    if first_slot
                        && matches!(
                            reply.frame.as_ref(),
                            Some(v1::ackplane_frame::Frame::ContextPacketUseAck(_))
                        )
                    {
                        if let Some(gate) = &reject_first_use {
                            let permit = tokio::select! {
                                _ = sender.closed() => return,
                                permit = gate.acquire() => permit.unwrap(),
                            };
                            permit.forget();
                            let _ = sender
                                .send(Ok(v1::AckplaneFrame {
                                    frame: Some(v1::ackplane_frame::Frame::Rejection(
                                        v1::Rejection {
                                            record_id: "first-slot-use".into(),
                                            reason: v1::RejectionReason::Malformed as i32,
                                            retryable: false,
                                            diagnostic: "injected first-slot delivery rejection"
                                                .into(),
                                        },
                                    )),
                                }))
                                .await;
                            return;
                        }
                    }
                }
                if let (Some(held), Ok(reply)) = (&held_contexts, &frame) {
                    if matches!(
                        reply.frame.as_ref(),
                        Some(v1::ackplane_frame::Frame::ContextPacketReply(_))
                    ) {
                        held.add_permits(1);
                        sender.closed().await;
                        return;
                    }
                }
                if sender.send(frame).await.is_err() {
                    return;
                }
            }
        });
        Ok(tonic::Response::new(ReceiverStream::new(receiver)))
    }
}

async fn wait_for<Value, Read, Reading>(label: &str, mut read: Read) -> Value
where
    Read: FnMut() -> Reading,
    Reading: Future<Output = Option<Value>>,
{
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Some(value) = read().await {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {label}"))
}

async fn assign(
    pool: &PgPool,
    tenant: &str,
    repository: &str,
    node: &str,
    session: &str,
    task_id: &str,
) {
    let service = WorkCommandService::connect(pool).await.unwrap();
    let payload = WorkCommandPayload::Assign(AssignPayload {
        target: DirectiveTarget {
            target_node_id: node.into(),
            target_session_id: session.into(),
        },
    });
    let authorization = WorkCommandAuthorization::Verified(VerifiedWorkCommandPrincipal {
        principal_id: "test-operator".into(),
        tenant_id: tenant.into(),
        repository_ids: vec![repository.into()],
        allowed_commands: vec![WorkCommandKind::Assign],
        policy_refs: vec!["constitution:v1".into()],
        delegation_id: None,
    });
    let now = SystemTime::now();
    let submitted = service
        .submit(
            authorization.clone(),
            NewWorkCommand {
                tenant_id: tenant.into(),
                repository_id: repository.into(),
                kind: WorkCommandKind::Assign,
                schema_version: "v1".into(),
                task_id: Some(task_id.into()),
                issuing_principal_id: "test-operator".into(),
                delegation_id: None,
                policy_refs: vec!["constitution:v1".into()],
                rationale: "Execute the addressed test task with scoped memory".into(),
                expected_task_version: Some(1),
                confirmation_id: None,
                expires_at: now + Duration::from_secs(300),
                idempotency_key: format!("assign:{task_id}"),
                payload_digest: payload_digest(&payload).unwrap(),
            },
            now,
        )
        .await
        .unwrap();
    let WorkCommandServiceOutcome::PendingConfirmation { command, .. } = submitted else {
        panic!("assignment did not request confirmation");
    };
    let confirmed = service
        .confirm(
            authorization,
            tenant,
            repository,
            &command.command_id,
            payload,
            now,
        )
        .await
        .unwrap();
    let WorkCommandServiceOutcome::Executed { receipt, .. } = confirmed else {
        panic!("assignment did not execute");
    };
    assert_eq!(receipt.outcome, WorkCommandOutcome::PendingDelivery);
}

#[tokio::test]
async fn server_runs_two_agents_with_separate_memory_prompts_and_durable_outcomes() {
    exercise_two_workers(Scenario::default()).await;
}

/// Stopping the supervisor used to abandon active processes and their durable receipts.
#[cfg(unix)]
#[tokio::test]
async fn shutdown_stops_both_workers_releases_leases_and_flushes_receipts() {
    exercise_two_workers(Scenario {
        stop_while_active: true,
        ..Scenario::default()
    })
    .await;
}

// Shutdown during context preparation used to report success but leave confirmed
// leases held for five minutes because no worker had become active yet.
#[cfg(unix)]
#[tokio::test]
async fn shutdown_before_context_delivery_releases_confirmed_leases_without_spawning() {
    exercise_two_workers(Scenario {
        stop_while_active: true,
        stop_before_spawn: true,
        ..Scenario::default()
    })
    .await;
}

// Completion cleared run markers after a release RPC failed, abandoning the
// tracked leases. Retry release before declaring that the slots are reusable.
#[tokio::test]
async fn failed_lease_release_is_retried_before_worker_cleanup_finishes() {
    exercise_two_workers(Scenario {
        release_failures: 2,
        ..Scenario::default()
    })
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn shutdown_retries_lease_release_before_discarding_run_markers() {
    exercise_two_workers(Scenario {
        stop_while_active: true,
        release_failures: 2,
        ..Scenario::default()
    })
    .await;
}

// A fatal slot result dropped the entire task group, bypassing active peers'
// lease release and terminal receipts. Drain peers before returning the failure.
#[tokio::test]
async fn a_failed_slot_drains_its_active_peer_before_supervisor_exit() {
    exercise_two_workers(Scenario {
        fail_first_slot: true,
        ..Scenario::default()
    })
    .await;
}

async fn exercise_two_workers(scenario: Scenario) {
    let Scenario {
        stop_while_active,
        stop_before_spawn,
        release_failures,
        fail_first_slot,
    } = scenario;
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let pool = build_pool(&database_url, TEST_POOL_MAX_SIZE).unwrap();
    let mut seed = [0_u8; 32];
    getrandom::getrandom(&mut seed).unwrap();
    let suffix = seed[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let tenant = format!("multi-agent:{suffix}");
    let repository = "repository:multi-agent";
    let node = "node:multi-agent";
    let signing_key_id = format!("key:{suffix}");
    let root = tempfile::tempdir().unwrap();
    let gate = root.path().join("finish");
    ackplane_server::enrollment_store::EnrollmentStore::connect(&pool)
        .await
        .unwrap();
    let ledger = LedgerStore::connect(&pool).await.unwrap();
    let context = ContextService::connect(&pool).await.unwrap();
    let supervisors = SupervisorStore::connect(&pool).await.unwrap();
    let work = WorkStore::connect(&pool).await.unwrap();
    {
        let key = ed25519_dalek::SigningKey::from_bytes(&seed);
        let public_key = key.verifying_key().to_bytes().to_vec();
        let mut connection = pool.get().await.unwrap();
        let transaction = connection.transaction().await.unwrap();
        signing_keys::register(
            &transaction,
            &SigningKeyRecord {
                signing_key_id: signing_key_id.clone(),
                tenant_id: tenant.clone(),
                repository_id: repository.into(),
                node_id: node.into(),
                public_key_fingerprint: ackplane_server::enrollment::public_key_fingerprint(
                    &public_key,
                ),
                public_key,
                activated_at: SystemTime::now() - Duration::from_secs(1),
                expires_at: None,
            },
        )
        .await
        .unwrap();
        transaction.commit().await.unwrap();
    }
    ConstitutionStore::connect(&pool)
        .await
        .unwrap()
        .publish(PublishConstitutionRequest {
            tenant_id: tenant.clone(),
            repository_id: repository.into(),
            version_id: "constitution:v1".into(),
            version: 1,
            status: "adopted".into(),
            clauses: vec![ClauseSnapshot {
                id: "goal:run".into(),
                slug: "run".into(),
                kind: "invariant".into(),
                title: "Evidence required".into(),
                statement: "Do not mark work complete without verification".into(),
                status: "active".into(),
                consequence: None,
                scope: None,
                rationale: None,
            }],
        })
        .await
        .unwrap();
    let knowledge = KnowledgeStore::connect(&pool).await.unwrap();
    let mut commands = BTreeMap::new();
    for name in ["first", "second"] {
        let directory = root.path().join(name);
        fs::create_dir(&directory).unwrap();
        commands.insert(
            name,
            WorkerCommand {
                command: env!("CARGO_BIN_EXE_prompt_worker").into(),
                args: vec!["{prompt}".into(), gate.to_string_lossy().into_owned()],
                working_directory: directory,
                branch: format!("agents/{name}"),
            },
        );
        work.create_task(
            &NewWorkTask {
                tenant_id: tenant.clone(),
                repository_id: repository.into(),
                task_id: format!("task:{name}"),
                title: format!("Run {name}"),
                acceptance: format!("The {name} worker writes its own prompt"),
                goal_id: Some("goal:run".into()),
                declared_paths: vec![format!("src/{name}.rs")],
                declared_symbols: vec![],
                published_by: node.into(),
            },
            &format!("created:{name}"),
            SystemTime::now(),
        )
        .await
        .unwrap();
        let lesson = knowledge
            .record(RecordKnowledgeRequest {
                tenant_id: tenant.clone(),
                repository_id: repository.into(),
                content: format!("Prior verified lesson for {name}"),
                source_ref: Some(format!("execution:{name}")),
                recorded_by: Some(node.into()),
                reach_node_ids: vec![format!("artifact:src/{name}.rs")],
                reach_goal_id: Some("goal:run".into()),
                half_life_hours: 168.0,
                embedding: None,
            })
            .await
            .unwrap();
        knowledge
            .activate(
                &tenant,
                repository,
                &lesson.knowledge_id,
                "test-operator",
                Some("verified fixture evidence"),
                SystemTime::now(),
            )
            .await
            .unwrap();
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let node_sync = NodeSyncService::with_supervisor_directive_and_work_store(
        ledger,
        SupervisorStore::connect(&pool).await.unwrap(),
        DirectiveStore::connect(&pool).await.unwrap(),
        WorkStore::connect(&pool).await.unwrap(),
        v1::FlowControl {
            max_in_flight_batches: 16,
            max_batch_bytes: 1_048_576,
        },
    )
    .with_context_service(context)
    .with_work_command_service(WorkCommandService::connect(&pool).await.unwrap());
    let held_contexts = stop_before_spawn.then(|| Arc::new(tokio::sync::Semaphore::new(0)));
    let reject_first_use = fail_first_slot.then(|| Arc::new(tokio::sync::Semaphore::new(0)));
    let node_sync = ContextReplyGate {
        service: node_sync,
        held_contexts: held_contexts.clone(),
        reject_first_use: reject_first_use.clone(),
    };
    let claims = ClaimDelegationService::new(ClaimStore::connect(&pool).await.unwrap());
    let query = WorkQueryService::new(WorkStore::connect(&pool).await.unwrap());
    let remaining_release_failures = Arc::new(AtomicUsize::new(0));
    let interceptor_failures = remaining_release_failures.clone();
    let (shutdown, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(v1::node_sync_service_server::NodeSyncServiceServer::new(
                node_sync,
            ))
            .add_service(
                v1::claim_delegation_service_server::ClaimDelegationServiceServer::with_interceptor(
                    claims,
                    ReleaseFailures(interceptor_failures),
                ),
            )
            .add_service(v1::work_query_service_server::WorkQueryServiceServer::new(
                query,
            ))
            .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let workers_file = root.path().join("workers.json");
    let daemon_log = root.path().join("daemon.log");
    fs::write(&workers_file, serde_json::to_vec(&commands).unwrap()).unwrap();
    let mut daemon = Daemon(
        Command::new(env!("CARGO_BIN_EXE_ackplane-supervisor"))
            .arg("--workers")
            .arg(&workers_file)
            .env("MINDLEAK_ACKPLANE_ENDPOINT", &endpoint)
            .env("MINDLEAK_ACKPLANE_TENANT_ID", &tenant)
            .env("MINDLEAK_ACKPLANE_REPOSITORY_ID", repository)
            .env("MINDLEAK_ACKPLANE_NODE_ID", node)
            .env("MINDLEAK_ACKPLANE_SIGNING_KEY_ID", &signing_key_id)
            .env(
                "MINDLEAK_ACKPLANE_NODE_SIGNING_KEY_SEED",
                ackplane_client::encode_seed(&seed),
            )
            .env_remove("MINDLEAK_ACKPLANE_TLS_CA_PATH")
            .env("ACKPLANE_SUPERVISOR_ID", "multi-agent")
            .env("ACKPLANE_SUPERVISOR_STATE_DIR", root.path().join("state"))
            .env("ACKPLANE_SUPERVISOR_HEARTBEAT_SECONDS", "1")
            .env_remove("ACKPLANE_SUPERVISOR_WORKERS")
            .env("RUST_LOG", "warn")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(if fail_first_slot {
                Stdio::from(fs::File::create(&daemon_log).unwrap())
            } else {
                Stdio::inherit()
            })
            .group_spawn()
            .unwrap(),
    );

    let mut sessions = Vec::new();
    for name in ["first", "second"] {
        let prefix = format!("multi-agent-{name}-");
        let session = wait_for("registered worker session", || async {
            for supervisor in supervisors
                .list_supervisors(&tenant, repository)
                .await
                .unwrap()
            {
                if supervisor.registration.supervisor_id.starts_with(&prefix) {
                    if let Some(session) = supervisors
                        .list_sessions(&tenant, repository, &supervisor.registration.supervisor_id)
                        .await
                        .unwrap()
                        .into_iter()
                        .next()
                    {
                        return Some(session.session);
                    }
                }
            }
            None
        })
        .await;
        assert!(
            session
                .supervisor_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
            "generated supervisor id must be safe as a queue filename"
        );
        assign(
            &pool,
            &tenant,
            repository,
            node,
            &session.session_id,
            &format!("task:{name}"),
        )
        .await;
        sessions.push(session);
    }
    if let Some(held_contexts) = held_contexts {
        let _held = tokio::time::timeout(Duration::from_secs(10), held_contexts.acquire_many(2))
            .await
            .expect("both context replies must be held before shutdown")
            .unwrap();
        let claims = ClaimStore::connect(&pool).await.unwrap();
        assert_eq!(
            claims
                .list_active(&tenant, repository, SystemTime::now())
                .await
                .unwrap()
                .len(),
            2
        );

        daemon.shutdown().await;

        assert!(
            claims
                .list_active(&tenant, repository, SystemTime::now())
                .await
                .unwrap()
                .is_empty(),
            "shutdown left confirmed preparation leases held without starting workers"
        );
        for (index, name) in ["first", "second"].iter().enumerate() {
            assert!(!root.path().join(name).join("prompt.json").exists());
            assert!(!root
                .path()
                .join("state")
                .join(format!("{name}.worker-run.json"))
                .exists());
            assert!(
                supervisors
                    .lifecycle_history(&tenant, repository, &sessions[index].session_id)
                    .await
                    .unwrap()
                    .is_empty(),
                "an unstarted worker must not receive fabricated lifecycle receipts"
            );
        }
        drop(daemon);
        let _ = shutdown.send(());
        server.await.unwrap();
        return;
    }
    wait_for("both workers to receive their prompts", || async {
        ["first", "second"]
            .iter()
            .all(|name| root.path().join(name).join("prompt.json").exists())
            .then_some(())
    })
    .await;
    for (index, name) in ["first", "second"].iter().enumerate() {
        let prompt: serde_json::Value =
            serde_json::from_slice(&fs::read(root.path().join(name).join("prompt.json")).unwrap())
                .unwrap();
        assert_eq!(prompt["scope"]["task_id"], format!("task:{name}"));
        let environment: serde_json::Value = serde_json::from_slice(
            &fs::read(root.path().join(name).join("environment.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            environment["inherited_control_settings"], false,
            "workers must not inherit control-plane credentials or database settings"
        );
        assert_eq!(
            prompt["scope"]["agent_session_id"],
            sessions[index].session_id
        );
        let context = prompt["context"].to_string();
        assert!(context.contains(&format!("Prior verified lesson for {name}")));
        let other = if *name == "first" { "second" } else { "first" };
        assert!(!context.contains(&format!("Prior verified lesson for {other}")));
        assert_eq!(prompt["mandatory"].as_array().unwrap().len(), 8);
        let packets = ContextPacketStore::connect(&pool).await.unwrap();
        let packet_id = prompt["packet_id"].as_str().unwrap();
        wait_for("durable context use receipt", || async {
            packets
                .list_use_receipts(&tenant, repository, packet_id)
                .await
                .unwrap()
                .iter()
                .any(|receipt| receipt.status == ContextPacketUseStatus::Accepted)
                .then_some(())
        })
        .await;
    }
    if let Some(reject_first_use) = reject_first_use {
        let peer = &sessions[1];
        wait_for("healthy peer startup receipt", || async {
            supervisors
                .lifecycle_history(&tenant, repository, &peer.session_id)
                .await
                .unwrap()
                .iter()
                .any(|entry| entry.receipt.state == SupervisorWorkerState::Started)
                .then_some(())
        })
        .await;
        reject_first_use.add_permits(1);
        let exit = daemon.wait().await;
        assert!(
            !exit.success(),
            "a fatal slot failure must not become a successful supervisor exit"
        );
        let diagnostic = fs::read_to_string(&daemon_log).unwrap();
        assert!(
            diagnostic.contains("injected first-slot delivery rejection"),
            "{diagnostic}"
        );
        let claims = ClaimStore::connect(&pool)
            .await
            .unwrap()
            .list_active(&tenant, repository, SystemTime::now())
            .await
            .unwrap();
        assert!(
            claims.iter().all(|claim| claim.task_id != "task:second"),
            "the failed slot aborted its healthy peer without releasing the peer lease"
        );
        let history = supervisors
            .lifecycle_history(&tenant, repository, &peer.session_id)
            .await
            .unwrap();
        assert_eq!(
            history.len(),
            2,
            "healthy peer shutdown must record exactly one terminal receipt"
        );
        assert!(history
            .iter()
            .any(|entry| entry.receipt.state == SupervisorWorkerState::Terminated));
        assert!(!root.path().join("state/second.worker-run.json").exists());
        assert!(
            root.path().join("state/first.worker-run.json").exists(),
            "the failed slot must retain its unacknowledged evidence"
        );
        drop(daemon);
        let _ = shutdown.send(());
        server.await.unwrap();
        return;
    }
    remaining_release_failures.store(release_failures, Ordering::SeqCst);
    let expected_state = if stop_while_active {
        daemon.shutdown().await;
        fs::write(&gate, "finish").unwrap();
        SupervisorWorkerState::Terminated
    } else {
        fs::write(&gate, "finish").unwrap();
        SupervisorWorkerState::Completed
    };
    for (index, name) in ["first", "second"].iter().enumerate() {
        wait_for("durable worker completion", || async {
            supervisors
                .lifecycle_history(&tenant, repository, &sessions[index].session_id)
                .await
                .unwrap()
                .iter()
                .any(|entry| entry.receipt.state == expected_state)
                .then_some(())
        })
        .await;
        let task = work
            .task_detail(&tenant, repository, &format!("task:{name}"))
            .await
            .unwrap()
            .unwrap()
            .task;
        assert_eq!(task.state, WorkTaskState::Claimed, "applied assignment must reach Work, while process exit is not verified task completion");
    }
    wait_for("acknowledged worker runs", || async {
        ["first", "second"]
            .iter()
            .all(|name| {
                !root
                    .path()
                    .join("state")
                    .join(format!("{name}.worker-run.json"))
                    .exists()
            })
            .then_some(())
    })
    .await;
    assert_eq!(
        remaining_release_failures.load(Ordering::SeqCst),
        0,
        "all injected release failures must be exercised"
    );
    assert!(
        ClaimStore::connect(&pool)
            .await
            .unwrap()
            .list_active(&tenant, repository, SystemTime::now())
            .await
            .unwrap()
            .is_empty(),
        "run markers were cleared while completed workers still held task leases"
    );
    for session in &sessions {
        let history = supervisors
            .lifecycle_history(&tenant, repository, &session.session_id)
            .await
            .unwrap();
        assert_eq!(
            history.len(),
            2,
            "cleanup retries must not invent additional lifecycle receipts"
        );
        assert_eq!(
            history
                .iter()
                .filter(|entry| entry.receipt.state == SupervisorWorkerState::Started)
                .count(),
            1
        );
        assert_eq!(
            history
                .iter()
                .filter(|entry| entry.receipt.state == expected_state)
                .count(),
            1
        );
    }
    drop(daemon);
    assert!(
        ["first", "second"].iter().all(|name| !root
            .path()
            .join("state")
            .join(format!("{name}.worker-run.json"))
            .exists()),
        "acknowledged runs must clear their recovery markers"
    );
    let _ = shutdown.send(());
    server.await.unwrap();
}

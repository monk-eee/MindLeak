//! Real browser commands, authenticated supervision, and an OS child in one isolated run.

#[allow(dead_code)]
mod supervisor_api_support;
mod work_command_browser_support;

use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ackplane_client::node_identity::{NodeIdentity, NodeSignerSource};
use ackplane_protocol::context_packet::ContextPacketUseStatus;
use ackplane_server::{
    claim_store::ClaimStore,
    constitution_store::{ClauseSnapshot, ConstitutionStore, PublishConstitutionRequest},
    context_packet_store::ContextPacketStore,
    db_pool::{build_pool, TEST_POOL_MAX_SIZE},
    supervisor_store::SupervisorStore,
};
use ackplane_supervisor::{config::SupervisorConfig, daemon, SupervisorOutbox, WorkerCommand};
use axum::http::{Method, StatusCode};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::watch,
};

use supervisor_api_support::{enroll_repository, unique_id};
use work_command_browser_support::{
    after_server_event, application, envelope, get_json, post_json, request, test_database_url,
    StopOnDrop, SyncServer, TestDirectory,
};

#[test]
fn work_command_prompt_fixture() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(argument) = arguments
        .last()
        .filter(|argument| argument.starts_with('{'))
    else {
        eprintln!(
            "worker fixture entrypoint; behavioral coverage comes from the runtime test's child"
        );
        return;
    };
    assert_eq!(
        &arguments[..3],
        ["--exact", "work_command_prompt_fixture", "--nocapture"]
    );
    assert_eq!(
        arguments.len(),
        4,
        "the JSON prompt is libtest's second OR filter"
    );
    let prompt: Value = serde_json::from_str(argument).expect("worker receives a JSON prompt");
    assert!(prompt["scope"]["task_id"].is_string());
    fs::write("prompt.json", argument).expect("child records its actual prompt");
    fs::write("worker.json", serde_json::to_vec(&json!({
        "pid": std::process::id(),
        "cwd": std::env::current_dir().unwrap(),
        "executable": std::env::current_exe().unwrap(),
        "inherited_control_environment": std::env::vars_os().any(|(name, _)| {
            let name = name.to_string_lossy().to_ascii_uppercase();
            name.starts_with("ACKPLANE_") || name.starts_with("MINDLEAK_ACKPLANE_")
                || name.starts_with("LODESTAR_") || matches!(name.as_str(), "DATABASE_URL" | "PGPASSWORD")
        }),
    })).unwrap()).expect("child records its process identity without credentials");
    let address = fs::read_to_string("release-address")
        .unwrap()
        .parse::<std::net::SocketAddr>()
        .unwrap();
    assert!(address.ip().is_loopback());
    let mut gate = TcpStream::connect_timeout(&address, Duration::from_secs(10)).unwrap();
    gate.set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    gate.set_write_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut release = [0];
    gate.read_exact(&mut release)
        .expect("the parent releases the inspected child");
    assert_eq!(release, [1]);
    fs::write("finished", "child finished").expect("record that the child passed its gate");
    gate.write_all(b"finished").unwrap();
}

#[tokio::test]
async fn browser_scoped_work_runs_real_supervisor_and_review_does_not_complete_task() {
    let Some(database_url) = test_database_url() else {
        return;
    };
    let root = TestDirectory::new();
    let unique = unique_id("work-runtime");
    let tenant_id = format!("tenant-{unique}");
    let repository_id = format!("repository-{unique}");
    let node_id = enroll_repository(&database_url, &tenant_id, &repository_id, &unique).await;
    let pool = build_pool(&database_url, TEST_POOL_MAX_SIZE).expect("build isolated runtime pool");
    let goal_id = format!("goal:{unique}");
    let constitution_id = format!("constitution:{unique}");
    let statement = "Run only the declared build scope and submit evidence for review";
    ConstitutionStore::connect(&pool)
        .await
        .unwrap()
        .publish(PublishConstitutionRequest {
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            version_id: constitution_id.clone(),
            version: 1,
            status: "adopted".into(),
            clauses: vec![ClauseSnapshot {
                id: goal_id.clone(),
                slug: "scoped-runtime".into(),
                kind: "objective".into(),
                title: "Verify scoped runtime work".into(),
                statement: statement.into(),
                status: "active".into(),
                consequence: None,
                scope: None,
                rationale: None,
            }],
        })
        .await
        .unwrap();
    let app = application(&database_url, &tenant_id)
        .await
        .merge(supervisor_api_support::application(&pool, &database_url, &tenant_id).await);
    let server = SyncServer::start(&pool).await;
    let gate = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let worker_directory = root.0.join("worker");
    fs::create_dir(&worker_directory).unwrap();
    fs::write(
        worker_directory.join("release-address"),
        gate.local_addr().unwrap().to_string(),
    )
    .unwrap();
    let config = SupervisorConfig {
        endpoint: server.endpoint.clone(),
        identity: NodeIdentity {
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            node_id: node_id.clone(),
            signing_key_id: format!("signing-key-{unique}"),
            signer_source: NodeSignerSource::Seed(Box::new(
                Sha256::digest(format!("key-{unique}").as_bytes()).into(),
            )),
        },
        supervisor_id: "bridge-runtime".into(),
        state_dir: root.0.join("state"),
        heartbeat_interval: Duration::from_millis(25),
        workers: [(
            "fixture".into(),
            WorkerCommand {
                command: std::env::current_exe().unwrap().to_str().unwrap().into(),
                args: vec![
                    "--exact".into(),
                    "work_command_prompt_fixture".into(),
                    "--nocapture".into(),
                    "{prompt}".into(),
                ],
                working_directory: worker_directory.clone(),
                branch: "agents/scoped-runtime-fixture".into(),
            },
        )]
        .into_iter()
        .collect(),
    };
    let (stop, stopping) = watch::channel(false);
    let scenario_config = config.clone();
    let scenario_pool = pool.clone();
    let mut changes = server.changes.clone();
    let scenario = tokio::spawn(async move {
        let _stop = StopOnDrop(stop);
        tokio::time::timeout(Duration::from_secs(40), async {
            let identity = &scenario_config.identity;
            let work_uri = format!("/api/v1/repositories/{}/work", identity.repository_id);
            let commands_uri = format!("{work_uri}/commands");
            let task_id = format!("task-{unique}");
            let task_uri = format!("{work_uri}/{task_id}");
            let paths = vec!["src/build.rs", "tests/build.rs"];
            let symbols = vec!["symbol:src/build.rs:build"];
            let payload = json!({
                "kind": "create_work", "task_id": task_id, "title": "Scoped browser runtime work",
                "acceptance": "A real child receives the exact scope and its exit is not task completion",
                "goal_id": goal_id, "declared_paths": paths, "declared_symbols": symbols,
            });
            let preview = post_json(&app, &commands_uri, envelope(payload.clone(), None)).await;
            assert_eq!(preview["status"], "pending_confirmation");
            assert_eq!(request(&app, Method::GET, &task_uri, None).await.status(), StatusCode::NOT_FOUND);
            let confirm_uri = format!("{commands_uri}/{}/confirm", preview["command_id"].as_str().unwrap());
            let created = post_json(&app, &confirm_uri, payload.clone()).await;
            assert_eq!(created["status"], "executed");
            assert_eq!(created["outcome"], "applied");
            let detail = get_json(&app, &task_uri).await;
            assert_eq!(detail["task"]["state"], "open");
            assert_eq!(detail["task"]["goal_id"], goal_id);
            assert_eq!(detail["task"]["declared_paths"], json!(paths));
            assert_eq!(detail["task"]["declared_symbols"], json!(symbols));
            let version = detail["task"]["version"].as_i64().unwrap();
            assert_eq!(version, 1);
            let listed = get_json(&app, &work_uri).await;
            assert_eq!(listed["total"], 1);
            assert_eq!(listed["items"][0]["task_id"], task_id);

            let supervisors_uri = format!("/api/v1/repositories/{}/supervisors", identity.repository_id);
            let (supervisor, session) = after_server_event(&mut changes, "assign-capable session through Bridge", || async {
                let inventory = get_json(&app, &supervisors_uri).await;
                for supervisor in inventory["entries"].as_array().unwrap() {
                    if supervisor["node_id"] != identity.node_id
                        || !supervisor["supported_directives"].as_array().unwrap().contains(&json!("assign")) {
                        continue;
                    }
                    let uri = format!("{supervisors_uri}/{}/sessions", supervisor["supervisor_id"].as_str().unwrap());
                    let sessions = get_json(&app, &uri).await;
                    if let Some(session) = sessions["entries"].as_array().unwrap().iter().find(|session| session["state"] == "started") {
                        return Some((supervisor.clone(), session.clone()));
                    }
                }
                None
            }).await;
            assert_eq!(supervisor["outbox_durability"], "persistent");
            assert_eq!(supervisor["recoverable_outbox"], true);
            assert_eq!(session["runtime"], "local_machine");
            let supervisor_id = supervisor["supervisor_id"].as_str().unwrap().to_owned();
            let session_id = session["session_id"].as_str().unwrap().to_owned();
            let worker_id = session["worker_id"].as_str().unwrap();
            let assignment = json!({"kind": "assign", "target_node_id": supervisor["node_id"], "target_session_id": session_id});
            let preview = post_json(&app, &commands_uri, envelope(assignment.clone(), Some((&task_id, version)))).await;
            assert_eq!(preview["status"], "pending_confirmation");
            assert_eq!(get_json(&app, &task_uri).await["task"]["state"], "open");
            assert!(!worker_directory.join("prompt.json").exists(), "preview must not spawn a child");
            let claims = ClaimStore::connect(&scenario_pool).await.unwrap();
            assert!(claims.list_active(&identity.tenant_id, &identity.repository_id, SystemTime::now()).await.unwrap().is_empty());
            let confirm_uri = format!("{commands_uri}/{}/confirm", preview["command_id"].as_str().unwrap());
            let assigned = post_json(&app, &confirm_uri, assignment).await;
            assert_eq!(assigned["status"], "executed");
            assert_eq!(assigned["outcome"], "pending_delivery");

            let (mut child, address) = tokio::time::timeout(Duration::from_secs(20), gate.accept()).await
                .expect("the real OS child must reach its loopback gate").unwrap();
            assert!(address.ip().is_loopback());
            let prompt: Value = serde_json::from_slice(&fs::read(worker_directory.join("prompt.json")).unwrap()).unwrap();
            let process: Value = serde_json::from_slice(&fs::read(worker_directory.join("worker.json")).unwrap()).unwrap();
            assert_ne!(process["pid"], json!(std::process::id()), "the prompt writer must be an OS child");
            assert_eq!(PathBuf::from(process["cwd"].as_str().unwrap()), worker_directory);
            assert_eq!(PathBuf::from(process["executable"].as_str().unwrap()), std::env::current_exe().unwrap());
            assert_eq!(process["inherited_control_environment"], false);
            assert_eq!(prompt["scope"], json!({
                "tenant_id": identity.tenant_id, "repository_id": identity.repository_id,
                "task_id": task_id, "goal_id": goal_id, "agent_session_id": session_id,
            }));
            after_server_event(&mut changes, "applied assignment", || async {
                let detail = get_json(&app, &task_uri).await;
                (detail["task"]["state"] == "claimed").then_some(detail)
            }).await;
            let active = claims.list_active(&identity.tenant_id, &identity.repository_id, SystemTime::now()).await.unwrap();
            assert_eq!(active.len(), 1);
            let lease = &active[0];
            assert_eq!(lease.task_id, task_id);
            assert_eq!(lease.owner_id, session_id);
            assert_eq!(lease.branch, scenario_config.workers["fixture"].branch);
            assert_eq!(lease.paths, paths);
            assert_eq!(lease.symbols, symbols);
            assert!(lease.lease_expires_at > SystemTime::now());
            let packets = ContextPacketStore::connect(&scenario_pool).await.unwrap();
            let packet_id = prompt["packet_id"].as_str().unwrap();
            let packet = packets.get_packet(&identity.tenant_id, &identity.repository_id, packet_id).await.unwrap().unwrap();
            packet.validate().unwrap();
            assert_eq!(json!(packet.scope), prompt["scope"]);
            assert_eq!(json!(packet.digest), prompt["packet_digest"]);
            assert_eq!(json!(packet.selected.iter().filter(|item| item.mandatory).collect::<Vec<_>>()), prompt["mandatory"]);
            assert_eq!(json!(packet.selected.iter().filter(|item| !item.mandatory).collect::<Vec<_>>()), prompt["context"]);
            let mandatory = prompt["mandatory"].as_array().unwrap();
            assert_eq!(mandatory.len(), 8);
            let rendered = |item_id: &str| mandatory.iter().find(|item| item["item_id"] == item_id).unwrap()["rendered"].as_str().unwrap();
            let rendered_lease: Value = serde_json::from_str(rendered(&format!("lease:{task_id}"))).unwrap();
            assert_eq!(rendered_lease, json!({
                "owner": session_id, "paths": paths, "symbols": symbols, "branch": lease.branch,
                "expires_at": lease.lease_expires_at.duration_since(UNIX_EPOCH).unwrap().as_secs(),
            }));
            assert_eq!(rendered(&format!("objective:{task_id}")), payload["title"].as_str().unwrap());
            assert_eq!(rendered(&format!("acceptance:{task_id}")), payload["acceptance"].as_str().unwrap());
            let policy: Value = serde_json::from_str(rendered(&format!("constitution:{constitution_id}"))).unwrap();
            assert_eq!(policy, json!([{"id": goal_id, "kind": "objective", "statement": statement, "scope": null, "consequence": null}]));
            let uses = packets.list_use_receipts(&identity.tenant_id, &identity.repository_id, packet_id).await.unwrap();
            assert_eq!(uses.len(), 1);
            assert_eq!(uses[0].status, ContextPacketUseStatus::Accepted);
            assert_eq!(uses[0].scope, packet.scope);

            let lifecycle_uri = format!("{supervisors_uri}/{supervisor_id}/sessions/{session_id}/lifecycle");
            let started = get_json(&app, &lifecycle_uri).await;
            assert_eq!(started["entries"].as_array().unwrap().len(), 1);
            assert_eq!(started["entries"][0]["state"], "started");
            assert_eq!(started["entries"][0]["worker_id"], worker_id);
            child.write_all(&[1]).await.unwrap();
            let mut reply = Vec::new();
            tokio::time::timeout(Duration::from_secs(10), child.read_to_end(&mut reply)).await.unwrap().unwrap();
            assert_eq!(reply, b"finished");
            let completed = after_server_event(&mut changes, "durable child completion", || async {
                let history = get_json(&app, &lifecycle_uri).await;
                (history["entries"].as_array().unwrap().last().map(|entry| &entry["state"]) == Some(&json!("completed"))).then_some(history)
            }).await;
            let entries = completed["entries"].as_array().unwrap();
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[1]["worker_id"], worker_id);
            assert!(entries[1]["receipt_position"].as_i64().unwrap() > entries[0]["receipt_position"].as_i64().unwrap());
            assert_eq!(fs::read_to_string(worker_directory.join("finished")).unwrap(), "child finished");
            let detail = get_json(&app, &task_uri).await;
            assert_eq!(detail["task"]["state"], "claimed", "process completion must not complete Work");
            let version = detail["task"]["version"].as_i64().unwrap();
            let review = json!({"kind": "submit_review", "disposition": "accept", "review_rationale": "The scoped child ran; conformance still decides task completion"});
            let preview = post_json(&app, &commands_uri, envelope(review.clone(), Some((&task_id, version)))).await;
            assert_eq!(preview["status"], "pending_confirmation");
            assert_eq!(get_json(&app, &task_uri).await["task"]["state"], "claimed");
            let confirm_uri = format!("{commands_uri}/{}/confirm", preview["command_id"].as_str().unwrap());
            let reviewed = post_json(&app, &confirm_uri, review).await;
            assert_eq!(reviewed["status"], "executed");
            assert_eq!(reviewed["outcome"], "applied");
            let detail = get_json(&app, &task_uri).await;
            assert_eq!(detail["task"]["state"], "in_review", "review acceptance is not verified completion");
            assert_eq!(detail["task"]["version"], version + 1);
            (supervisor_id, session_id)
        }).await.expect("the runtime scenario has a bounded deadline")
    });
    let (daemon_result, scenario_result) = tokio::join!(
        daemon::run(&config, Duration::from_millis(25), stopping),
        scenario,
    );
    server.shutdown().await;
    assert!(
        daemon_result.is_ok(),
        "supervisor failed: {daemon_result:?}; scenario: {scenario_result:?}"
    );
    let (supervisor_id, session_id) =
        scenario_result.expect("runtime assertions failed after orderly shutdown");
    assert!(
        !config.worker_run_path("fixture").exists(),
        "the child must be reaped and its run marker cleared"
    );
    assert!(
        ClaimStore::connect(&pool)
            .await
            .unwrap()
            .list_active(&tenant_id, &repository_id, SystemTime::now())
            .await
            .unwrap()
            .is_empty(),
        "no test lease may remain active"
    );
    let supervisors = SupervisorStore::connect(&pool).await.unwrap();
    let registration = supervisors
        .list_supervisors(&tenant_id, &repository_id)
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.registration.supervisor_id == supervisor_id)
        .unwrap()
        .registration;
    let session = supervisors
        .list_sessions(&tenant_id, &repository_id, &supervisor_id)
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.session.session_id == session_id)
        .unwrap()
        .session;
    let outbox = SupervisorOutbox::open(
        config.state_dir.join(format!("{supervisor_id}.outbox.db")),
        registration,
        session,
    )
    .unwrap();
    let positions = outbox.positions().unwrap();
    assert!(positions.last_enqueued >= 4);
    assert_eq!(positions.acknowledged, positions.last_enqueued);
    assert!(outbox.pending(10).unwrap().is_empty());
    drop(outbox);
    let directory = root.0.clone();
    drop(root);
    assert!(
        !directory.exists(),
        "test-owned directories must be removed"
    );
}

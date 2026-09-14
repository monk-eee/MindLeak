use std::{
    process::{Command, Output},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use ackplane_bridge::{
    design_api::{design_routes, DesignApiState},
    work_command_api::{work_command_routes, WorkCommandApiState},
};
use ackplane_server::{
    constitution_store::{ClauseSnapshot, ConstitutionStore, RecordConstitutionPublicationRequest},
    design_materialization_store::MaterializationStore,
    design_store::DesignStore,
    fleet::FleetStore,
    work_command_store::WorkCommandService,
    work_store::WorkStore,
};
use serde_json::Value;

#[path = "../../ackplane-bridge/tests/support/design_enrollment.rs"]
mod design_enrollment;

struct Bridge {
    url: String,
    repository: String,
    server: tokio::task::JoinHandle<()>,
}

impl Bridge {
    async fn command(&self, arguments: &[&str]) -> Output {
        let arguments: Vec<String> = arguments
            .iter()
            .map(|value| value.to_string())
            .chain([
                "--bridge-url".into(),
                self.url.clone(),
                "--repository-id".into(),
                self.repository.clone(),
            ])
            .collect();
        tokio::task::spawn_blocking(move || {
            Command::new(env!("CARGO_BIN_EXE_ackplane-workctl"))
                .args(arguments)
                .output()
                .unwrap()
        })
        .await
        .unwrap()
    }

    async fn json(&self, arguments: &[&str]) -> Value {
        let output = self.command(arguments).await;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.server.abort();
    }
}

#[tokio::test]
async fn conversation_design_is_persisted_and_only_explicitly_confirmed_work_is_created() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let unique = format!(
        "conversation-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let tenant = format!("tenant-{unique}");
    let repository = format!("repository-{unique}");
    design_enrollment::enroll_repository(&database_url, &tenant, &repository, &unique).await;
    let pool = ackplane_server::db_pool::build_pool(
        &database_url,
        ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
    )
    .unwrap();
    let designs = Arc::new(DesignStore::connect(&pool).await.unwrap());
    let materializations = Arc::new(MaterializationStore::connect(&pool).await.unwrap());
    let fleet = Arc::new(FleetStore::connect(&pool).await.unwrap());
    let commands = Arc::new(WorkCommandService::connect(&pool).await.unwrap());
    let work = WorkStore::connect(&pool).await.unwrap();
    let app = design_routes(DesignApiState::new(
        designs.clone(),
        materializations,
        fleet.clone(),
        Arc::from(tenant.as_str()),
    ))
    .merge(work_command_routes(WorkCommandApiState::new(
        commands,
        fleet,
        Arc::from(tenant.as_str()),
    )));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut bridge = Bridge {
        url: format!("http://{}", listener.local_addr().unwrap()),
        repository: repository.clone(),
        server: tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }),
    };
    let file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/conversation_design.json"
    );
    let design_id = "design:retry-receipts";
    let preview = bridge.json(&["design", "preview", "--file", file]).await;
    assert!(designs
        .get_design(&tenant, &repository, design_id)
        .await
        .unwrap()
        .is_none());
    let digest = preview["confirmation_digest"].as_str().unwrap();
    let published = bridge
        .json(&[
            "design",
            "propose",
            "--file",
            file,
            "--confirm-digest",
            digest,
        ])
        .await;
    assert_eq!(published["persisted"], true);
    assert_eq!(published["design"]["design"]["lifecycle_state"], "proposed");
    let replay = bridge
        .json(&[
            "design",
            "propose",
            "--file",
            file,
            "--confirm-digest",
            digest,
        ])
        .await;
    assert_eq!(replay, published);
    let listed = bridge
        .json(&["design", "list", "--state", "proposed"])
        .await;
    assert_eq!(listed["total"], 1);
    let decision = bridge
        .json(&[
            "design",
            "decision-preview",
            "--design-id",
            design_id,
            "--decision",
            "accepted",
            "--rationale",
            "Operator approved this bounded design",
        ])
        .await;
    assert_eq!(decision["persisted"], false);
    let decided = bridge
        .json(&[
            "design",
            "decide",
            "--design-id",
            design_id,
            "--decision",
            "accepted",
            "--rationale",
            "Operator approved this bounded design",
            "--confirm-digest",
            decision["confirmation_digest"].as_str().unwrap(),
        ])
        .await;
    assert_eq!(decided["design"]["lifecycle_state"], "accepted");
    assert_eq!(decided["decisions"][0]["actor"], tenant);
    let stale = bridge
        .command(&[
            "design",
            "decide",
            "--design-id",
            design_id,
            "--decision",
            "accepted",
            "--rationale",
            "Operator approved this bounded design",
            "--confirm-digest",
            decision["confirmation_digest"].as_str().unwrap(),
        ])
        .await;
    assert!(!stale.status.success());
    assert!(String::from_utf8_lossy(&stale.stderr).contains("confirmation does not match"));
    assert!(work
        .task_detail(&tenant, &repository, "task:conversation")
        .await
        .unwrap()
        .is_none());
    // The CLI formerly required the operator's private Bridge principal string,
    // preventing a normal conversation from handing an accepted design to Work.
    for (task_id, path) in [
        ("task:conversation", "src/receipts.rs"),
        ("task:consumer", "src/consumer.rs"),
    ] {
        let payload = [
            "--task-id",
            task_id,
            "--title",
            "Preserve receipts",
            "--acceptance",
            "Reconnect regression passes",
            "--path",
            path,
        ];
        let submit: Vec<&str> = [
            "submit",
            "create_work",
            "--idempotency-key",
            task_id,
            "--rationale",
            "Implement design:retry-receipts",
            "--expires-in-seconds",
            "600",
        ]
        .into_iter()
        .chain(payload)
        .collect();
        let pending = bridge.json(&submit).await;
        assert_eq!(pending["status"], "pending_confirmation");
        assert!(work
            .task_detail(&tenant, &repository, task_id)
            .await
            .unwrap()
            .is_none());
        let confirm: Vec<&str> = [
            "confirm",
            "create_work",
            "--command-id",
            pending["command_id"].as_str().unwrap(),
        ]
        .into_iter()
        .chain(payload)
        .collect();
        let confirmed = bridge.json(&confirm).await;
        assert_eq!(confirmed["outcome"], "applied");
        let task = work
            .task_detail(&tenant, &repository, task_id)
            .await
            .unwrap()
            .unwrap()
            .task;
        assert_eq!(task.declared_paths, vec![path]);
        assert_eq!(task.acceptance, "Reconnect regression passes");
    }
    let forged = bridge
        .json(&[
            "submit",
            "create_work",
            "--issuing-principal-id",
            "not-the-bridge-principal",
            "--idempotency-key",
            "forged-work",
            "--rationale",
            "Must be refused",
            "--expires-in-seconds",
            "600",
            "--task-id",
            "task:forged",
            "--title",
            "Refuse this",
            "--acceptance",
            "Never created",
        ])
        .await;
    assert_eq!(forged["status"], "refused");
    assert_eq!(forged["reason"], "forged_principal");
    assert!(work
        .task_detail(&tenant, &repository, "task:forged")
        .await
        .unwrap()
        .is_none());
    let constitution = "constitution:conversation-v1";
    ConstitutionStore::connect(&pool)
        .await
        .unwrap()
        .record_publication(RecordConstitutionPublicationRequest {
            tenant_id: tenant.clone(),
            repository_id: repository,
            version_id: constitution.into(),
            schema_version: "v1".into(),
            status: "active".into(),
            clauses: vec![ClauseSnapshot {
                id: "receipts".into(),
                slug: "receipts".into(),
                kind: "constraint".into(),
                title: "Retain receipts".into(),
                statement: "A lost acknowledgement cannot discard evidence".into(),
                status: "active".into(),
                consequence: None,
                scope: None,
                rationale: None,
            }],
            source_reference: None,
            source_digest: None,
            published_at: SystemTime::now(),
        })
        .await
        .unwrap();
    let link_arguments = [
        "--design-id",
        design_id,
        "--constitution-version-id",
        constitution,
        "--work-task-id",
        "task:conversation",
        "--work-task-id",
        "task:consumer",
        "--idempotency-key",
        "conversation-link-v1",
        "--rationale",
        "Confirmed task implements the accepted design",
    ];
    let preview_arguments: Vec<&str> = ["design", "materialization-preview"]
        .into_iter()
        .chain(link_arguments)
        .collect();
    let preview = bridge.json(&preview_arguments).await;
    assert_eq!(preview["persisted"], false);
    let digest = preview["confirmation_digest"].as_str().unwrap();
    let link_arguments: Vec<&str> = ["design", "materialize"]
        .into_iter()
        .chain(link_arguments)
        .chain(["--confirm-digest", digest])
        .collect();
    let linked = bridge.json(&link_arguments).await;
    assert_eq!(linked["persisted"], true);
    assert_eq!(bridge.json(&link_arguments).await, linked);
    let detail = bridge
        .json(&["design", "show", "--design-id", design_id])
        .await;
    assert_eq!(detail["materializations"].as_array().unwrap().len(), 1);
    assert_eq!(
        detail["materializations"][0]["work_task_ids"],
        serde_json::json!(["task:consumer", "task:conversation"])
    );
    assert_eq!(detail["materializations"][0]["actor"], tenant);
    assert_eq!(detail["design"]["lifecycle_state"], "accepted");
    let mut changed: Value =
        serde_json::from_str(include_str!("fixtures/conversation_design.json")).unwrap();
    changed["summary"] = Value::String("Cannot overwrite the accepted design".into());
    let changed_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(changed_file.path(), serde_json::to_vec(&changed).unwrap()).unwrap();
    let path = changed_file.path().to_str().unwrap();
    let review = bridge.json(&["design", "preview", "--file", path]).await;
    let conflict = bridge
        .command(&[
            "design",
            "propose",
            "--file",
            path,
            "--confirm-digest",
            review["confirmation_digest"].as_str().unwrap(),
        ])
        .await;
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stderr).contains("HTTP 409"));
    assert_eq!(
        bridge
            .json(&["design", "show", "--design-id", design_id])
            .await,
        detail
    );
    let foreign = format!("foreign-{unique}");
    design_enrollment::enroll_repository(&database_url, &foreign, &foreign, &foreign).await;
    bridge.repository = foreign;
    let invisible = bridge.command(&["design", "list"]).await;
    assert!(!invisible.status.success());
    assert!(String::from_utf8_lossy(&invisible.stderr).contains("HTTP 404"));
}

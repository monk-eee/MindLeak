//! Browser command coverage against the real Bridge routes and isolated stores.

#[allow(dead_code)]
mod supervisor_api_support;

use std::sync::Arc;

use ackplane_bridge::{
    work_api::{work_routes, WorkApiState},
    work_command_api::{work_command_routes, WorkCommandApiState},
};
use ackplane_server::{
    db_pool::{build_pool, TEST_POOL_MAX_SIZE},
    fleet::FleetStore,
    work_command_store::WorkCommandService,
    work_store::WorkStore,
};
use axum::{
    body::{to_bytes, Body},
    http::{header::CONTENT_TYPE, Method, Request, StatusCode},
    response::Response,
    Router,
};
use serde_json::{json, Value};
use supervisor_api_support::{body_json, enroll_repository, unique_id};
use tower::ServiceExt;

async fn application(database_url: &str, tenant_id: &str) -> Router {
    let pool = build_pool(database_url, TEST_POOL_MAX_SIZE).expect("build isolated test pool");
    let work = Arc::new(WorkStore::connect(&pool).await.expect("connect Work store"));
    let fleet = Arc::new(
        FleetStore::connect(&pool)
            .await
            .expect("connect Fleet store"),
    );
    let commands = Arc::new(
        WorkCommandService::connect(&pool)
            .await
            .expect("connect Work command service"),
    );
    work_routes(WorkApiState::new(work, fleet.clone(), Arc::from(tenant_id))).merge(
        work_command_routes(WorkCommandApiState::new(
            commands,
            fleet,
            Arc::from(tenant_id),
        )),
    )
}

async fn request(router: &Router, method: Method, uri: &str, body: Option<Value>) -> Response {
    router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(CONTENT_TYPE, "application/json")
                .body(body.map_or_else(Body::empty, |value| Body::from(value.to_string())))
                .expect("build browser request"),
        )
        .await
        .expect("serve browser request")
}

async fn post_json(router: &Router, uri: &str, body: Value) -> Value {
    let response = request(router, Method::POST, uri, Some(body.clone())).await;
    assert_eq!(response.status(), StatusCode::OK, "POST {uri}: {body}");
    body_json(response).await
}

fn envelope(mut payload: Value, existing_task: Option<(&str, i64)>) -> Value {
    payload["idempotency_key"] = json!(unique_id("browser-command"));
    payload["rationale"] = json!("Exercise the browser command contract");
    payload["expires_at_seconds"] = json!(4_000_000_000_u64);
    if let Some((task_id, version)) = existing_task {
        payload["existing_task_id"] = json!(task_id);
        payload["expected_task_version"] = json!(version);
    }
    payload
}

/// Preview retries previously changed an immutable receipt's timestamp and
/// returned HTTP 500. Reuse the recorded command time while preserving guards.
#[tokio::test]
async fn browser_work_commands_replay_without_receipt_conflicts_and_preserve_guards() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("not run: ACKPLANE_TEST_DATABASE_URL must point to isolated ackplane_test");
        return;
    };
    let config = database_url
        .parse::<tokio_postgres::Config>()
        .unwrap_or_else(|_| panic!("the test database configuration must be valid"));
    assert_eq!(
        config.get_dbname(),
        Some("ackplane_test"),
        "browser integration tests must never use the live database"
    );

    let unique = unique_id("work-command-browser");
    let tenant_id = format!("tenant-{unique}");
    let repository_id = format!("repository-{unique}");
    enroll_repository(&database_url, &tenant_id, &repository_id, &unique).await;
    let app = application(&database_url, &tenant_id).await;
    let work_uri = format!("/api/v1/repositories/{repository_id}/work");
    let commands_uri = format!("{work_uri}/commands");
    let task_id = format!("task-{unique}");
    let task_uri = format!("{work_uri}/{task_id}");

    for (uri, content_type, expected_body) in [
        (
            "/static/work-command-client.mjs",
            "text/javascript; charset=utf-8",
            include_str!("../static/work-command-client.mjs"),
        ),
        (
            "/static/work-page.mjs",
            "text/javascript; charset=utf-8",
            include_str!("../static/work-page.mjs"),
        ),
        (
            "/static/work-page.css",
            "text/css; charset=utf-8",
            include_str!("../static/work-page.css"),
        ),
    ] {
        let response = request(&app, Method::GET, uri, None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], content_type);
        let body = to_bytes(response.into_body(), 1_048_576)
            .await
            .expect("read bounded static asset");
        assert_eq!(body.as_ref(), expected_body.as_bytes());
        assert_eq!(
            request(&app, Method::POST, uri, None).await.status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
    }
    assert_eq!(
        request(&app, Method::GET, "/static/not-an-asset.mjs", None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );

    let create_payload = json!({
        "kind": "create_work",
        "task_id": task_id,
        "title": "Browser-created work",
        "acceptance": "A confirmed browser request creates exactly one task",
    });
    let submission = envelope(create_payload.clone(), None);
    let preview = post_json(&app, &commands_uri, submission.clone()).await;
    assert_eq!(preview["status"], json!("pending_confirmation"));
    assert_eq!(preview["outcome"], json!("pending_confirmation"));
    assert_eq!(preview["idempotent_replay"], json!(false));
    assert!(!preview.to_string().contains(&tenant_id));
    assert_eq!(
        request(&app, Method::GET, &task_uri, None).await.status(),
        StatusCode::NOT_FOUND,
        "a preview must not create the task"
    );
    let mut expected_replay = preview.clone();
    expected_replay["idempotent_replay"] = json!(true);
    assert_eq!(
        post_json(&app, &commands_uri, submission).await,
        expected_replay
    );

    for (field, value, reason) in [
        ("issuing_principal_id", json!("forged"), "forged_principal"),
        ("issuing_principal_id", json!(""), "forged_principal"),
        ("policy_refs", json!(["unadopted"]), "policy_not_permitted"),
        (
            "delegation_id",
            json!("ungranted"),
            "delegation_not_permitted",
        ),
    ] {
        let mut refused = envelope(create_payload.clone(), None);
        refused[field] = value;
        assert_eq!(
            post_json(&app, &commands_uri, refused).await,
            json!({"status": "refused", "reason": reason})
        );
    }
    let mut explicit = envelope(create_payload.clone(), None);
    explicit["issuing_principal_id"] = json!(tenant_id);
    let explicit_preview = post_json(&app, &commands_uri, explicit).await;
    assert_eq!(explicit_preview["status"], json!("pending_confirmation"));
    assert!(!explicit_preview.to_string().contains(&tenant_id));

    let confirm_uri = format!(
        "{commands_uri}/{}/confirm",
        preview["command_id"].as_str().expect("preview command id")
    );
    let applied = post_json(&app, &confirm_uri, create_payload.clone()).await;
    assert_eq!(applied["status"], json!("executed"));
    assert_eq!(applied["outcome"], json!("applied"));
    assert_eq!(applied["idempotent_replay"], json!(false));
    assert!(!applied.to_string().contains(&tenant_id));
    let mut expected_replay = applied;
    expected_replay["idempotent_replay"] = json!(true);
    assert_eq!(
        post_json(&app, &confirm_uri, create_payload).await,
        expected_replay
    );

    let detail = body_json(request(&app, Method::GET, &task_uri, None).await).await;
    let version = detail["task"]["version"]
        .as_i64()
        .expect("detail must expose the authoritative task version");
    assert_eq!(version, 1);
    assert_eq!(detail["task"]["state"], json!("open"));
    let list = body_json(request(&app, Method::GET, &work_uri, None).await).await;
    assert_eq!(list["total"], json!(1));
    assert_eq!(list["items"][0]["version"], json!(version));

    let route_payload = json!({"kind": "route_work", "route_reference": "route-a"});
    let mismatch = post_json(
        &app,
        &commands_uri,
        envelope(route_payload.clone(), Some((&task_id, version))),
    )
    .await;
    let mismatch_uri = format!(
        "{commands_uri}/{}/confirm",
        mismatch["command_id"].as_str().expect("preview command id")
    );
    let mismatched = post_json(
        &app,
        &mismatch_uri,
        json!({"kind": "route_work", "route_reference": "changed-after-preview"}),
    )
    .await;
    assert_eq!(mismatched["outcome"], json!("refused"));
    assert!(mismatched["reason"].as_str().unwrap().contains("digest"));
    let detail = body_json(request(&app, Method::GET, &task_uri, None).await).await;
    assert_eq!(detail["task"]["version"], json!(version));

    let first = post_json(
        &app,
        &commands_uri,
        envelope(route_payload.clone(), Some((&task_id, version))),
    )
    .await;
    let stale = post_json(
        &app,
        &commands_uri,
        envelope(route_payload.clone(), Some((&task_id, version))),
    )
    .await;
    for (preview, outcome) in [(first, "applied"), (stale, "conflicted")] {
        let uri = format!(
            "{commands_uri}/{}/confirm",
            preview["command_id"].as_str().expect("preview command id")
        );
        let receipt = post_json(&app, &uri, route_payload.clone()).await;
        assert_eq!(receipt["outcome"], json!(outcome));
    }
    let detail = body_json(request(&app, Method::GET, &task_uri, None).await).await;
    assert_eq!(detail["task"]["version"], json!(version + 1));
    assert_eq!(detail["task"]["state"], json!("open"));
}

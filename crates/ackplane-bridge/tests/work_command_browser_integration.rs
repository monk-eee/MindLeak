//! Browser command coverage against the real Bridge routes and isolated stores.

#[allow(dead_code)]
mod supervisor_api_support;
#[allow(dead_code)]
mod work_command_browser_support;

use axum::{
    body::to_bytes,
    http::{header::CONTENT_TYPE, Method, StatusCode},
};
use serde_json::json;
use supervisor_api_support::{body_json, enroll_repository, unique_id};
use work_command_browser_support::{application, envelope, post_json, request, test_database_url};

/// Preview retries previously changed an immutable receipt's timestamp and
/// returned HTTP 500. Reuse the recorded command time while preserving guards.
#[tokio::test]
async fn browser_work_commands_replay_without_receipt_conflicts_and_preserve_guards() {
    let Some(database_url) = test_database_url() else {
        return;
    };

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
        "goal_id": "goal:browser-build",
        "declared_paths": ["src/build.rs", "tests/build.rs"],
        "declared_symbols": ["symbol:src/build.rs:build"],
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

    let changed_scope_uri = format!(
        "{commands_uri}/{}/confirm",
        explicit_preview["command_id"]
            .as_str()
            .expect("preview command id")
    );
    let mut changed_scope = create_payload.clone();
    changed_scope["declared_paths"] = json!(["outside-the-preview.rs"]);
    let refused_scope = post_json(&app, &changed_scope_uri, changed_scope).await;
    assert_eq!(refused_scope["outcome"], json!("refused"));
    assert!(refused_scope["reason"].as_str().unwrap().contains("digest"));
    assert_eq!(
        request(&app, Method::GET, &task_uri, None).await.status(),
        StatusCode::NOT_FOUND
    );

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
    assert_eq!(detail["task"]["goal_id"], json!("goal:browser-build"));
    assert_eq!(
        detail["task"]["declared_paths"],
        json!(["src/build.rs", "tests/build.rs"])
    );
    assert_eq!(
        detail["task"]["declared_symbols"],
        json!(["symbol:src/build.rs:build"])
    );
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

use ackplane_server::claim_store::{ClaimLeaseRequest, ClaimOwner};
use axum::{
    body::{to_bytes, Body},
    http::Request,
    routing::post,
    Router,
};
use serde_json::json;
use tower::ServiceExt;

use super::super::tests::{enroll_repository, test_app_state, unique_id};
use super::*;

#[tokio::test]
async fn operator_recovery_names_the_destination_node_and_preserves_live_and_tenant_guards() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        return;
    };
    let unique = unique_id("node-recovery");
    let tenant = format!("tenant-{unique}");
    let repository = format!("repository-{unique}");
    enroll_repository(&database_url, &tenant, &repository, &unique).await;
    let state = test_app_state(&database_url, &tenant).await;
    state
        .claims
        .delegate(
            &ClaimLeaseRequest {
                tenant_id: tenant.clone(),
                repository_id: repository.clone(),
                task_id: "task".into(),
                owner_id: "prior-owner".into(),
                node_id: "prior-node".into(),
                branch: "agents/prior".into(),
                lease: Duration::from_secs(60),
                paths: vec!["src/task.rs".into()],
                symbols: vec![],
            },
            SystemTime::now() - Duration::from_secs(120),
        )
        .await
        .unwrap();
    let path = format!("/api/v1/repositories/{repository}/tasks/task/recover");
    let router = Router::new()
        .route(
            "/api/v1/repositories/:repository_id/tasks/:task_id/recover",
            post(repository_recover_claim),
        )
        .with_state(state.clone());
    let request = |body: serde_json::Value| {
        Request::builder()
            .method("POST")
            .uri(&path)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let body = json!({"owner_id":"next-owner", "branch":"agents/next", "reason":"recover a stranded fixture", "lease_seconds":60});
    let missing = router.clone().oneshot(request(body.clone())).await.unwrap();
    assert_eq!(missing.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let mut blank = body.clone();
    blank["node_id"] = json!("  ");
    let blank = router.clone().oneshot(request(blank)).await.unwrap();
    assert_eq!(blank.status(), StatusCode::BAD_REQUEST);
    let before = state
        .fleet
        .claim_owner(&tenant, &repository, "task")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before.owner_id, "prior-owner");
    let node_id = format!("node-{unique}");
    let mut body = body;
    body["node_id"] = json!(node_id);
    let response = router.clone().oneshot(request(body.clone())).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 64 * 1024).await.unwrap()).unwrap();
    assert_eq!(response["node_id"], node_id);
    let claim = state
        .claims
        .list_active(&tenant, &repository, SystemTime::now())
        .await
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(claim.owner_id, "next-owner");
    assert_eq!(claim.node_id.as_deref(), Some(node_id.as_str()));
    assert!(matches!(
        state
            .claims
            .release(
                &tenant,
                &repository,
                "task",
                ClaimOwner {
                    owner_id: "next-owner",
                    node_id: "prior-node"
                },
                SystemTime::now()
            )
            .await,
        Err(ClaimStoreError::OwnerNodeMismatch)
    ));
    let live = router.oneshot(request(body.clone())).await.unwrap();
    assert_eq!(live.status(), StatusCode::CONFLICT);
    let foreign = test_app_state(&database_url, &format!("foreign-{tenant}")).await;
    let foreign = Router::new()
        .route(
            "/api/v1/repositories/:repository_id/tasks/:task_id/recover",
            post(repository_recover_claim),
        )
        .with_state(foreign);
    assert_eq!(
        foreign.oneshot(request(body)).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    assert!(state
        .claims
        .release(
            &tenant,
            &repository,
            "task",
            ClaimOwner {
                owner_id: "next-owner",
                node_id: &node_id
            },
            SystemTime::now()
        )
        .await
        .unwrap());
}

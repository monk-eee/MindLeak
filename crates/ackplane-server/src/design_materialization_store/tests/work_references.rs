use super::*;

#[tokio::test]
async fn unsorted_work_references_remain_idempotent_and_match_readback() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let fixture = build_fixture(&database_url).await;
    let pool = crate::test_support::gated_test_pool();
    let work = WorkStore::connect(&pool).await.unwrap();
    for task_id in ["task:z", "task:a", "task:Z"] {
        work.create_task(
            &NewWorkTask {
                tenant_id: fixture.tenant_id.clone(),
                repository_id: fixture.repository_id.clone(),
                task_id: task_id.into(),
                title: task_id.into(),
                acceptance: "A referenced task exists".into(),
                goal_id: None,
                declared_paths: vec![],
                declared_symbols: vec![],
                published_by: "test".into(),
            },
            &format!("event:{task_id}"),
            SystemTime::now(),
        )
        .await
        .unwrap();
    }
    let store = MaterializationStore::connect(&pool).await.unwrap();
    let mut request = request(&fixture, "unordered-tasks");
    request.work_task_ids = vec!["task:z".into(), "task:a".into(), "task:Z".into()];
    // The junction table reads task IDs in sorted order. Comparing that result
    // with unsorted input turned an identical retry into a false conflict.
    let first = store.record_materialization(request.clone()).await.unwrap();
    let replay = store.record_materialization(request.clone()).await.unwrap();
    assert_eq!(first, replay);
    assert_eq!(first.work_task_ids, vec!["task:Z", "task:a", "task:z"]);
    request.work_task_ids.reverse();
    assert_eq!(
        store.record_materialization(request.clone()).await.unwrap(),
        first
    );
    request.work_task_ids.pop();
    assert!(matches!(
        store.record_materialization(request).await,
        Err(MaterializationStoreError::IdempotencyConflict { .. })
    ));
    let history = store
        .list_materializations(
            &fixture.tenant_id,
            &fixture.repository_id,
            &fixture.design_id,
        )
        .await
        .unwrap();
    assert_eq!(history, vec![first]);
}

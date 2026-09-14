use std::{sync::Arc, time::Duration};

use tokio::{task::JoinSet, time::timeout};

use super::*;

async fn lock_design<'client>(
    controller: &'client mut PgConnection,
    fixture: &Fixture,
) -> (deadpool_postgres::Transaction<'client>, i32) {
    let controller_pid: i32 = controller
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .unwrap()
        .get(0);
    let gate = controller.transaction().await.unwrap();
    gate.query_one(
        "SELECT design_id FROM industrial_designs \
         WHERE tenant_id = $1 AND repository_id = $2 AND design_id = $3 FOR UPDATE",
        &[
            &fixture.tenant_id,
            &fixture.repository_id,
            &fixture.design_id,
        ],
    )
    .await
    .unwrap();
    (gate, controller_pid)
}

async fn wait_for_blocked(observer: &PgConnection, controller_pid: i32, writer_count: i64) {
    timeout(Duration::from_secs(10), async {
        let mut interval = tokio::time::interval(Duration::from_millis(10));
        loop {
            let blocked: i64 = observer
                .query_one(
                    "WITH RECURSIVE waiters(pid) AS ( \
                         SELECT pid FROM pg_stat_activity WHERE $1 = ANY(pg_blocking_pids(pid)) \
                         UNION \
                         SELECT activity.pid FROM pg_stat_activity AS activity \
                         JOIN waiters ON waiters.pid = ANY(pg_blocking_pids(activity.pid)) \
                     ) SELECT COUNT(*) FROM waiters",
                    &[&controller_pid],
                )
                .await
                .unwrap()
                .get(0);
            if blocked >= writer_count {
                break;
            }
            interval.tick().await;
        }
    })
    .await
    .expect("every writer must be observed waiting on the held design lock");
}

async fn blocked_writers(
    database_url: &str,
    fixture: &Fixture,
    requests: Vec<RecordMaterializationRequest>,
) -> Vec<Result<MaterializationRevision, MaterializationStoreError>> {
    let pool = crate::db_pool::build_pool(database_url, requests.len()).unwrap();
    let store = Arc::new(MaterializationStore::connect(&pool).await.unwrap());
    let control_pool = crate::db_pool::build_pool(database_url, 2).unwrap();
    let mut controller = crate::db_pool::checkout(&control_pool).await.unwrap();
    let observer = crate::db_pool::checkout(&control_pool).await.unwrap();
    let (gate, controller_pid) = lock_design(&mut controller, fixture).await;
    let writer_count = requests.len() as i64;
    let mut writers = JoinSet::new();
    for request in requests {
        let store = Arc::clone(&store);
        writers.spawn(async move { store.record_materialization(request).await });
    }
    wait_for_blocked(&observer, controller_pid, writer_count).await;
    gate.commit().await.unwrap();
    timeout(Duration::from_secs(10), async {
        let mut results = Vec::new();
        while let Some(result) = writers.join_next().await {
            results.push(result.unwrap());
        }
        results
    })
    .await
    .expect("writers finish after the gate releases")
}

#[tokio::test]
async fn concurrent_identical_materializations_return_the_same_receipt() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let fixture = build_fixture(&database_url).await;
    let request = request(&fixture, "concurrent-identical");
    // Both writers used to pass the retry check and allocate revision one;
    // the loser then returned a database error for an identical valid request.
    let results = blocked_writers(&database_url, &fixture, vec![request; 8]).await;
    let revisions: Vec<_> = results.into_iter().map(Result::unwrap).collect();
    assert!(revisions.iter().all(|revision| revision == &revisions[0]));
    assert_eq!(revisions[0].revision_number, 1);
    let store = MaterializationStore::connect(&crate::test_support::gated_test_pool())
        .await
        .unwrap();
    let history = store
        .list_materializations(
            &fixture.tenant_id,
            &fixture.repository_id,
            &fixture.design_id,
        )
        .await
        .unwrap();
    assert_eq!(history, vec![revisions[0].clone()]);
}

#[tokio::test]
async fn concurrent_distinct_materializations_allocate_unique_revisions() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let fixture = build_fixture(&database_url).await;
    // Concurrent operators must not compete for MAX(revision_number) + 1.
    let results = blocked_writers(
        &database_url,
        &fixture,
        (0..8)
            .map(|index| request(&fixture, &format!("concurrent-{index}")))
            .collect(),
    )
    .await;
    let mut revisions: Vec<_> = results.into_iter().map(Result::unwrap).collect();
    revisions.sort_by_key(|revision| revision.revision_number);
    assert_eq!(
        revisions
            .iter()
            .map(|revision| revision.revision_number)
            .collect::<Vec<_>>(),
        (1..=8).collect::<Vec<_>>()
    );
    assert_eq!(
        revisions
            .iter()
            .map(|revision| &revision.idempotency_key)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        8
    );
}

#[tokio::test]
async fn concurrent_conflicting_materializations_report_the_typed_conflict() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let fixture = build_fixture(&database_url).await;
    let first = request(&fixture, "concurrent-conflict");
    let mut second = first.clone();
    second.rationale = Some("A different materialization decision".into());
    // A reused key with changed content must be a domain conflict, not an
    // incidental unique-constraint failure caused by racing inserts.
    let results = blocked_writers(&database_url, &fixture, vec![first, second]).await;
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(
                result,
                Err(MaterializationStoreError::IdempotencyConflict { .. })
            ))
            .count(),
        1,
        "{results:?}"
    );
}

#[tokio::test]
async fn failed_references_roll_back_and_retries_use_one_connection() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let fixture = build_fixture(&database_url).await;
    let pool = crate::db_pool::build_pool(&database_url, 1).unwrap();
    pool.resize(1);
    let store = MaterializationStore::connect(&pool).await.unwrap();
    timeout(Duration::from_secs(10), async {
        let original = request(&fixture, "rollback-retry");
        let mut invalid_constitution = original.clone();
        invalid_constitution.constitution_version_id = "constitution:missing".into();
        let mut invalid_task = original.clone();
        invalid_task.work_task_ids = vec!["task:missing".into()];
        for invalid in [invalid_constitution, invalid_task] {
            let error = store.record_materialization(invalid).await.unwrap_err();
            assert!(
                matches!(error, MaterializationStoreError::Database(ref database)
                if database.code() == Some(&tokio_postgres::error::SqlState::FOREIGN_KEY_VIOLATION))
            );
            assert!(store
                .list_materializations(
                    &fixture.tenant_id,
                    &fixture.repository_id,
                    &fixture.design_id
                )
                .await
                .unwrap()
                .is_empty());
        }
        let first = store
            .record_materialization(original.clone())
            .await
            .unwrap();
        assert_eq!(first.revision_number, 1);
        assert_eq!(store.record_materialization(original).await.unwrap(), first);
        assert_eq!(
            store
                .get_materialization(
                    &fixture.tenant_id,
                    &fixture.repository_id,
                    &fixture.design_id,
                    1
                )
                .await
                .unwrap(),
            Some(first.clone())
        );
        assert_eq!(
            store
                .list_materializations(
                    &fixture.tenant_id,
                    &fixture.repository_id,
                    &fixture.design_id
                )
                .await
                .unwrap(),
            vec![first]
        );
    })
    .await
    .expect("rollback and retry cannot require another pool slot");
    assert_eq!(pool.status().size, 1);
}

#[tokio::test]
async fn cancelling_a_blocked_writer_does_not_write_and_the_same_request_can_retry() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let fixture = build_fixture(&database_url).await;
    let pool = crate::db_pool::build_pool(&database_url, 1).unwrap();
    pool.resize(1);
    let store = Arc::new(MaterializationStore::connect(&pool).await.unwrap());
    let control_pool = crate::db_pool::build_pool(&database_url, 2).unwrap();
    let mut controller = crate::db_pool::checkout(&control_pool).await.unwrap();
    let observer = crate::db_pool::checkout(&control_pool).await.unwrap();
    let (gate, controller_pid) = lock_design(&mut controller, &fixture).await;
    let original = request(&fixture, "cancelled-retry");
    let mut writers = JoinSet::new();
    let worker_store = Arc::clone(&store);
    let request = original.clone();
    writers.spawn(async move { worker_store.record_materialization(request).await });
    wait_for_blocked(&observer, controller_pid, 1).await;
    writers.abort_all();
    assert!(writers
        .join_next()
        .await
        .unwrap()
        .unwrap_err()
        .is_cancelled());
    gate.commit().await.unwrap();
    timeout(Duration::from_secs(10), async {
        assert!(store
            .list_materializations(
                &fixture.tenant_id,
                &fixture.repository_id,
                &fixture.design_id
            )
            .await
            .unwrap()
            .is_empty());
        let revision = store
            .record_materialization(original.clone())
            .await
            .unwrap();
        assert_eq!(revision.revision_number, 1);
        assert_eq!(
            store.record_materialization(original).await.unwrap(),
            revision
        );
    })
    .await
    .expect("cancelled writer releases its transaction and connection");
}

#[tokio::test]
async fn a_locked_design_does_not_block_other_designs_repositories_or_tenants() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let fixture = build_fixture(&database_url).await;
    let mut independent = Vec::new();
    for (tenant, repository, design) in [
        (
            fixture.tenant_id.clone(),
            fixture.repository_id.clone(),
            unique_id("other-design"),
        ),
        (
            fixture.tenant_id.clone(),
            unique_id("other-repository"),
            fixture.design_id.clone(),
        ),
        (
            unique_id("other-tenant"),
            fixture.repository_id.clone(),
            fixture.design_id.clone(),
        ),
    ] {
        independent.push(Fixture::create(&database_url, tenant, repository, design).await);
    }
    let pool = crate::db_pool::build_pool(&database_url, 4).unwrap();
    let store = Arc::new(MaterializationStore::connect(&pool).await.unwrap());
    let control_pool = crate::db_pool::build_pool(&database_url, 2).unwrap();
    let mut controller = crate::db_pool::checkout(&control_pool).await.unwrap();
    let observer = crate::db_pool::checkout(&control_pool).await.unwrap();
    let (gate, controller_pid) = lock_design(&mut controller, &fixture).await;
    let mut writers = JoinSet::new();
    let worker_store = Arc::clone(&store);
    let waiting = request(&fixture, "scope-check");
    writers.spawn(async move { worker_store.record_materialization(waiting).await });
    wait_for_blocked(&observer, controller_pid, 1).await;
    timeout(Duration::from_secs(10), async {
        for scope in independent {
            let original = request(&scope, "scope-check");
            let revision = store
                .record_materialization(original.clone())
                .await
                .unwrap();
            assert_eq!(revision.revision_number, 1);
            assert_eq!(
                store.record_materialization(original).await.unwrap(),
                revision
            );
        }
    })
    .await
    .expect("another design, repository or tenant cannot wait on this row lock");
    gate.commit().await.unwrap();
    assert_eq!(
        timeout(Duration::from_secs(10), writers.join_next())
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .unwrap()
            .revision_number,
        1
    );
}

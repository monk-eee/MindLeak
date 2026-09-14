use super::{tests::new_task, WorkStore, WorkTaskState};
use crate::test_support::{test_pool, unique_id};
use std::time::{Duration, SystemTime};

// An empty page lost the window count and falsely reported no matching tasks.
#[tokio::test]
async fn a_page_beyond_the_end_retains_the_matching_task_total() {
    let Some(pool) = test_pool() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let tenant = unique_id("work-page-tenant");
    let repository = unique_id("work-page-repository");
    let store = WorkStore::connect(&pool).await.expect("connect Work store");
    store
        .create_task(
            &new_task(&tenant, &repository, "task", "Keep the total"),
            &unique_id("event"),
            SystemTime::now(),
        )
        .await
        .expect("create task");

    for state in [None, Some(WorkTaskState::Open)] {
        for requested_page in [2, i64::MAX] {
            let page = store
                .list_tasks(&tenant, &repository, state, requested_page, 20)
                .await
                .expect("read an empty page");
            assert!(page.items.is_empty());
            assert_eq!(
                page.total, 1,
                "an out-of-range page must not erase the matching task total"
            );
        }
    }
}

// A valid positive Bridge page could overflow its offset and panic the request.
#[tokio::test]
async fn a_large_positive_page_is_empty_instead_of_panicking() {
    let Some(pool) = test_pool() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let store = WorkStore::connect(&pool).await.expect("connect Work store");
    let page = store
        .list_tasks(
            &unique_id("large-page-tenant"),
            "repository",
            None,
            i64::MAX,
            20,
        )
        .await
        .expect("read an empty large page");
    assert!(page.items.is_empty());
    assert_eq!(page.total, 0);
}

#[tokio::test]
async fn work_pages_preserve_order_filters_and_repository_boundaries() {
    let Some(pool) = test_pool() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let tenant = unique_id("ordered-page-tenant");
    let foreign_tenant = unique_id("foreign-page-tenant");
    let repository = "repository";
    let store = WorkStore::connect(&pool).await.expect("connect Work store");
    let now = SystemTime::now();
    for (task_id, created_at) in [
        ("task-b", now),
        ("task-a", now),
        ("newest", now + Duration::from_secs(1)),
    ] {
        store
            .create_task(
                &new_task(&tenant, repository, task_id, "Ordered task"),
                &unique_id("event"),
                created_at,
            )
            .await
            .expect("create page task");
    }
    for (hidden_tenant, hidden_repository) in [
        (foreign_tenant.as_str(), repository),
        (tenant.as_str(), "other-repository"),
    ] {
        store
            .create_task(
                &new_task(hidden_tenant, hidden_repository, "newest", "Hidden task"),
                &unique_id("event"),
                now,
            )
            .await
            .expect("create a task outside the requested scope");
    }

    for state in [None, Some(WorkTaskState::Open)] {
        for (requested_page, expected_ids) in [
            (1, vec!["newest", "task-a"]),
            (2, vec!["task-b"]),
            (3, vec![]),
        ] {
            let page = store
                .list_tasks(&tenant, repository, state, requested_page, 2)
                .await
                .expect("read a scoped ordered page");
            assert_eq!(page.total, 3);
            assert_eq!(
                page.items
                    .iter()
                    .map(|task| task.task_id.as_str())
                    .collect::<Vec<_>>(),
                expected_ids
            );
            assert!(page.items.iter().all(|task| {
                task.tenant_id == tenant
                    && task.repository_id == repository
                    && task.state == WorkTaskState::Open
            }));
        }
    }

    for (empty_repository, state) in [
        (repository, Some(WorkTaskState::Completed)),
        ("empty-repository", None),
    ] {
        let page = store
            .list_tasks(&tenant, empty_repository, state, 1, 2)
            .await
            .expect("read a list with no matching tasks");
        assert_eq!(page.total, 0);
        assert!(page.items.is_empty());
    }
}

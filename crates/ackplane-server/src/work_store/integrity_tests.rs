use super::{
    tests::new_task, WorkDoctorFinding, WorkIntegrityReason, WorkStore, WorkStoreError,
    WorkTaskState,
};
use crate::test_support::{test_pool, unique_id};
use std::time::SystemTime;

// Work lists exposed tasks whose projection no longer identified its source event.
#[tokio::test]
async fn a_task_without_its_source_event_is_unavailable_to_ordinary_reads() {
    let Some(pool) = test_pool() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let tenant = unique_id("work-integrity");
    let repository = "repository";
    let store = WorkStore::connect(&pool).await.expect("connect Work store");
    store
        .create_task(
            &new_task(&tenant, repository, "task", "Consistent Work"),
            "event",
            SystemTime::now(),
        )
        .await
        .expect("create a valid task");
    pool.get()
        .await
        .expect("checkout fixture connection")
        .execute(
            "UPDATE work_tasks SET source_event_position = NULL \
             WHERE tenant_id = $1 AND repository_id = $2 AND task_id = 'task'",
            &[&tenant, &repository],
        )
        .await
        .expect("corrupt only the test task projection");

    let result = store.list_tasks(&tenant, repository, None, 1, 20).await;

    assert!(
        result.is_err(),
        "inconsistent Work must not be returned as a current task list"
    );
    assert!(
        store
            .task_detail(&tenant, repository, "task")
            .await
            .is_err(),
        "inconsistent Work must not be returned as ordinary task detail"
    );
    assert!(
        store
            .publication(&tenant, repository, SystemTime::now())
            .await
            .is_err(),
        "inconsistent Work must not be reported as current publication"
    );
    assert_eq!(
        store
            .board_doctor(
                &tenant,
                repository,
                SystemTime::now(),
                std::time::Duration::from_secs(3600)
            )
            .await
            .expect("integrity failures must remain diagnosable"),
        vec![WorkDoctorFinding::InconsistentProjection {
            task_id: "task".to_owned(),
            reason: WorkIntegrityReason::MissingSourcePosition,
        }],
        "Board Doctor must identify repair work for an inconsistent projection"
    );
}

// A repository maximum can hide corruption in an older task, including a pointer
// to another task's event. Filters and empty pages must not hide it either.
#[tokio::test]
async fn each_task_is_checked_against_its_own_latest_scoped_event_without_repairing_it() {
    let Some(pool) = test_pool() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let store = WorkStore::connect(&pool).await.expect("connect Work store");
    for (corruption, reason) in [
        (
            "UPDATE work_tasks SET source_event_position = NULL",
            WorkIntegrityReason::MissingSourcePosition,
        ),
        (
            "DELETE FROM work_task_history",
            WorkIntegrityReason::MissingHistory,
        ),
        (
            "UPDATE work_tasks SET source_event_position = 1",
            WorkIntegrityReason::EventPositionMismatch,
        ),
        (
            "UPDATE work_tasks SET source_event_position = 3",
            WorkIntegrityReason::EventPositionMismatch,
        ),
        (
            "UPDATE work_tasks SET source_event_position = 99",
            WorkIntegrityReason::EventPositionMismatch,
        ),
        (
            "UPDATE work_tasks SET state = 3",
            WorkIntegrityReason::StateMismatch,
        ),
    ] {
        let tenant = unique_id("per-task-integrity");
        let other_tenant = unique_id("other-task-integrity");
        let repository = "repository";
        for task_id in ["older", "affected", "anchor"] {
            store
                .create_task(
                    &new_task(&tenant, repository, task_id, task_id),
                    task_id,
                    SystemTime::now(),
                )
                .await
                .expect("create scoped task and its event");
        }
        for (scope_tenant, scope_repository) in [
            (tenant.as_str(), "other-repository"),
            (other_tenant.as_str(), repository),
        ] {
            store
                .create_task(
                    &new_task(scope_tenant, scope_repository, "affected", "Foreign task"),
                    "affected",
                    SystemTime::now(),
                )
                .await
                .expect("create identically named task in another scope");
        }
        let connection = pool.get().await.expect("checkout corruption fixture");
        connection.execute(
            "INSERT INTO work_task_waits (tenant_id, repository_id, wait_id, task_id, question, asked_by, asked_at) \
             VALUES ($1,$2,'wait','affected','Continue?','asker',$3)",
            &[&tenant, &repository, &SystemTime::UNIX_EPOCH],
        ).await.expect("seed an overdue wait on the affected task");
        assert_eq!(connection.execute(
            &format!("{corruption} WHERE tenant_id = $1 AND repository_id = $2 AND task_id = 'affected'"),
            &[&tenant, &repository],
        ).await.expect("corrupt only the older test task"), 1);
        let before = connection.query_one(
            "SELECT source_event_position, state, \
                (SELECT COUNT(*) FROM work_task_history history \
                 WHERE history.tenant_id = task.tenant_id AND history.repository_id = task.repository_id \
                   AND history.task_id = task.task_id) AS history_count \
             FROM work_tasks task WHERE tenant_id = $1 AND repository_id = $2 AND task_id = 'affected'",
            &[&tenant, &repository],
        ).await.expect("capture corrupted fixture before reads");
        drop(connection);

        for (state, page) in [
            (None, 1),
            (Some(WorkTaskState::Completed), 1),
            (None, i64::MAX),
        ] {
            let error = store
                .list_tasks(&tenant, repository, state, page, 20)
                .await
                .expect_err("filtered and empty pages cannot hide corruption");
            assert!(
                matches!(error, WorkStoreError::InconsistentProjection { task_id, reason: actual, .. }
                if task_id == "affected" && actual == reason),
                "{corruption}"
            );
        }
        let error = store
            .task_detail(&tenant, repository, "affected")
            .await
            .expect_err("corrupt detail must be withheld");
        assert!(
            matches!(error, WorkStoreError::InconsistentProjection { task_id, reason: actual, .. }
            if task_id == "affected" && actual == reason)
        );
        let error = store
            .fleet_unanswered_waits(
                &tenant,
                SystemTime::now(),
                std::time::Duration::from_secs(3600),
                20,
            )
            .await
            .expect_err("tenant-wide waits cannot expose or hide inconsistent tasks");
        assert!(
            matches!(error, WorkStoreError::InconsistentProjection { task_id, reason: actual, repository_id }
            if task_id == "affected" && actual == reason && repository_id == repository)
        );
        assert!(store
            .fleet_unanswered_waits(
                &other_tenant,
                SystemTime::now(),
                std::time::Duration::from_secs(3600),
                20
            )
            .await
            .expect("unrelated tenant waits remain readable")
            .is_empty());
        assert_eq!(
            store
                .board_doctor(
                    &tenant,
                    repository,
                    SystemTime::now(),
                    std::time::Duration::from_secs(3600)
                )
                .await
                .expect("diagnose unavailable scope"),
            vec![WorkDoctorFinding::InconsistentProjection {
                task_id: "affected".to_owned(),
                reason,
            }]
        );
        assert!(store
            .task_detail(&tenant, repository, "anchor")
            .await
            .expect("an unaffected task detail remains readable")
            .is_some());
        for (scope_tenant, scope_repository) in [
            (tenant.as_str(), "other-repository"),
            (other_tenant.as_str(), repository),
        ] {
            let page = store
                .list_tasks(scope_tenant, scope_repository, None, 1, 20)
                .await
                .expect("another scope must remain available");
            assert_eq!(page.total, 1);
            assert_eq!(page.publication.state(), "current");
            assert_eq!(page.items[0].task_id, "affected");
        }

        let after = pool.get().await.expect("checkout verification connection")
            .query_one(
                "SELECT source_event_position, state, \
                    (SELECT COUNT(*) FROM work_task_history history \
                     WHERE history.tenant_id = task.tenant_id AND history.repository_id = task.repository_id \
                       AND history.task_id = task.task_id) AS history_count \
                 FROM work_tasks task WHERE tenant_id = $1 AND repository_id = $2 AND task_id = 'affected'",
                &[&tenant, &repository],
            ).await.expect("verify no implicit repair");
        assert_eq!(
            before.get::<_, Option<i64>>("source_event_position"),
            after.get::<_, Option<i64>>("source_event_position")
        );
        assert_eq!(before.get::<_, i16>("state"), after.get::<_, i16>("state"));
        assert_eq!(
            before.get::<_, i64>("history_count"),
            after.get::<_, i64>("history_count")
        );
    }
}

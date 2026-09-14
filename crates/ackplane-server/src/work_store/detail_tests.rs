use std::time::{Duration, SystemTime};

use super::{tests::new_task, WorkStore};
use crate::{
    db_pool::{build_pool, TEST_POOL_MAX_SIZE},
    test_support::{unique_id, uuid_ish},
    work_command_store::{
        payload_digest, AnswerWaitPayload, NewWorkCommand, VerifiedWorkCommandPrincipal,
        WorkCommandAuthorization, WorkCommandKind, WorkCommandOutcome, WorkCommandPayload,
        WorkCommandService, WorkCommandServiceOutcome,
    },
};

// Separate queries mixed an old task with newly committed history and answers.
// A single read snapshot must retain all three until the next detail request.
#[tokio::test]
async fn a_concurrent_answer_does_not_mix_work_detail_versions() {
    let Ok(url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let pool = build_pool(&url, TEST_POOL_MAX_SIZE).expect("build test pool");
    let writer = WorkStore::connect(&pool).await.expect("connect Work store");
    let service = WorkCommandService::connect(&pool)
        .await
        .expect("connect command service");
    let tenant = unique_id("work-detail-snapshot");
    let repository = "repository";
    let task_id = "task";
    let wait_id = "wait";
    let now = SystemTime::now();
    writer
        .create_task(
            &new_task(&tenant, repository, task_id, "Consistent detail"),
            &unique_id("event"),
            now,
        )
        .await
        .expect("create task");
    pool.get()
        .await
        .expect("checkout wait fixture connection")
        .execute(
            "INSERT INTO work_task_waits \
                 (tenant_id, repository_id, wait_id, task_id, question, asked_by) \
             VALUES ($1,$2,$3,$4,'Proceed?','asker')",
            &[&tenant, &repository, &wait_id, &task_id],
        )
        .await
        .expect("seed unanswered wait");
    let baseline = writer
        .task_detail(&tenant, repository, task_id)
        .await
        .expect("read baseline")
        .expect("task exists");
    let payload = WorkCommandPayload::AnswerWait(AnswerWaitPayload {
        wait_id: wait_id.to_owned(),
        answer: "Approved".to_owned(),
    });
    let authorization = WorkCommandAuthorization::Verified(VerifiedWorkCommandPrincipal {
        principal_id: "operator".to_owned(),
        tenant_id: tenant.clone(),
        repository_ids: vec![repository.to_owned()],
        allowed_commands: vec![WorkCommandKind::AnswerWait],
        policy_refs: vec!["policy".to_owned()],
        delegation_id: Some("delegation".to_owned()),
    });
    let submitted = service
        .submit(
            authorization.clone(),
            NewWorkCommand {
                tenant_id: tenant.clone(),
                repository_id: repository.to_owned(),
                kind: WorkCommandKind::AnswerWait,
                schema_version: "v1".to_owned(),
                task_id: Some(task_id.to_owned()),
                issuing_principal_id: "operator".to_owned(),
                delegation_id: Some("delegation".to_owned()),
                policy_refs: vec!["policy".to_owned()],
                rationale: "Answer while a task detail read is in progress".to_owned(),
                expected_task_version: Some(1),
                confirmation_id: None,
                expires_at: now + Duration::from_secs(600),
                idempotency_key: unique_id("answer"),
                payload_digest: payload_digest(&payload).expect("digest answer"),
            },
            now,
        )
        .await
        .expect("submit answer");
    let WorkCommandServiceOutcome::PendingConfirmation { command, .. } = submitted else {
        panic!("answer must require confirmation");
    };

    let read_pool = build_pool(&url, 1).expect("build reader pool");
    read_pool.resize(1);
    let reader = WorkStore::connect(&read_pool)
        .await
        .expect("connect reader");
    let read_connection = read_pool.get().await.expect("checkout reader connection");
    let reader_pid: i32 = read_connection
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .expect("identify reader")
        .get(0);
    let barrier_key = uuid_ish() as i64;
    read_connection
        .batch_execute(&format!(
            "CREATE FUNCTION pg_temp.pause_work_detail() RETURNS boolean \
             LANGUAGE plpgsql VOLATILE AS $$ BEGIN \
                 PERFORM pg_advisory_xact_lock({barrier_key}::bigint); \
                 RETURN true; \
             END $$; \
             CREATE TEMP VIEW work_tasks AS \
                 SELECT * FROM public.work_tasks \
                 WHERE tenant_id = '{tenant}' AND pg_temp.pause_work_detail()"
        ))
        .await
        .expect("install connection-local read barrier");
    drop(read_connection);
    let mut barrier_connection = pool.get().await.expect("checkout barrier connection");
    let barrier = barrier_connection
        .transaction()
        .await
        .expect("start barrier");
    barrier
        .query_one("SELECT pg_advisory_xact_lock($1)", &[&barrier_key])
        .await
        .expect("hold barrier");
    let observer = pool.get().await.expect("checkout observer");
    let (detail, ()) = tokio::time::timeout(Duration::from_secs(20), async {
        tokio::join!(reader.task_detail(&tenant, repository, task_id), async {
            loop {
                let waiting: bool = observer
                    .query_one(
                        "SELECT EXISTS (SELECT 1 FROM pg_locks \
                         WHERE pid = $1 AND locktype = 'advisory' AND NOT granted)",
                        &[&reader_pid],
                    )
                    .await
                    .expect("observe blocked reader")
                    .get(0);
                if waiting {
                    break;
                }
                tokio::task::yield_now().await;
            }
            let executed = service
                .confirm(
                    authorization,
                    &tenant,
                    repository,
                    &command.command_id,
                    payload,
                    now,
                )
                .await
                .expect("confirm answer while detail is blocked");
            let WorkCommandServiceOutcome::Executed { receipt, .. } = executed else {
                panic!("the confirmed answer must execute");
            };
            assert_eq!(receipt.outcome, WorkCommandOutcome::Applied);
            barrier.commit().await.expect("release reader");
        })
    })
    .await
    .expect("the controlled task detail read must not hang");

    read_pool
        .get()
        .await
        .expect("checkout reader for cleanup")
        .batch_execute("DROP VIEW pg_temp.work_tasks; DROP FUNCTION pg_temp.pause_work_detail()")
        .await
        .expect("remove connection-local barrier");
    let detail = detail
        .expect("read task detail")
        .expect("task still exists");
    eprintln!(
        "detail snapshot: version={}, history={}, answered={}",
        detail.task.version,
        detail.history.len(),
        detail.waits[0].answer.is_some()
    );
    assert_eq!(detail.task, baseline.task);
    assert_eq!(
        detail.history, baseline.history,
        "history must match the task snapshot"
    );
    assert_eq!(
        detail.waits, baseline.waits,
        "waits must match the task snapshot"
    );

    let latest = reader
        .task_detail(&tenant, repository, task_id)
        .await
        .expect("read after the command committed")
        .expect("task still exists");
    assert_eq!(latest.task.version, 2);
    assert_eq!(latest.history.len(), 2);
    assert_eq!(
        latest.task.source_event_position,
        Some(latest.history[0].stream_position)
    );
    assert_eq!(latest.waits[0].answer.as_deref(), Some("Approved"));
    assert_eq!(latest.waits[0].answered_by.as_deref(), Some("operator"));
    assert!(latest.waits[0].answered_at.is_some());
}

#[tokio::test]
async fn task_detail_keeps_scope_and_releases_the_snapshot_for_missing_tasks() {
    let Ok(url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let pool = build_pool(&url, 1).expect("build single-connection pool");
    pool.resize(1);
    let store = WorkStore::connect(&pool).await.expect("connect Work store");
    let tenant = unique_id("detail-scope");
    let other_tenant = unique_id("other-detail-scope");
    let now = SystemTime::now();
    for (task_tenant, repository, title) in [
        (tenant.as_str(), "repository", "Requested task"),
        (tenant.as_str(), "other-repository", "Other repository"),
        (other_tenant.as_str(), "repository", "Other tenant"),
    ] {
        store
            .create_task(
                &new_task(task_tenant, repository, "shared-task-id", title),
                "shared-event-id",
                now,
            )
            .await
            .expect("create scoped task");
        pool.get()
            .await
            .expect("checkout wait fixture connection")
            .execute(
                "INSERT INTO work_task_waits \
                     (tenant_id, repository_id, wait_id, task_id, question, asked_by) \
                 VALUES ($1,$2,'shared-wait-id','shared-task-id',$3,'asker')",
                &[&task_tenant, &repository, &title],
            )
            .await
            .expect("seed scoped wait");
    }

    tokio::time::timeout(Duration::from_secs(5), async {
        for (task_tenant, repository, task_id) in [
            (tenant.as_str(), "repository", "missing-task"),
            (tenant.as_str(), "missing-repository", "shared-task-id"),
            (other_tenant.as_str(), "other-repository", "shared-task-id"),
        ] {
            assert!(store
                .task_detail(task_tenant, repository, task_id)
                .await
                .expect("missing task read succeeds")
                .is_none());
        }
        let detail = store
            .task_detail(&tenant, "repository", "shared-task-id")
            .await
            .expect("reuse the single connection")
            .expect("requested task exists");
        assert_eq!(detail.task.tenant_id, tenant);
        assert_eq!(detail.task.repository_id, "repository");
        assert_eq!(detail.task.title, "Requested task");
        assert_eq!(detail.history.len(), 1);
        assert_eq!(detail.history[0].stream_position, 1);
        assert_eq!(detail.waits.len(), 1);
        assert_eq!(detail.waits[0].question, "Requested task");
    })
    .await
    .expect("missing task reads must release their single pooled connection");
}

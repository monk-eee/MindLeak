use std::time::{Duration, SystemTime};

use ackplane_protocol::v1::{self, work_query_service_server::WorkQueryService as _};
use tonic::Request;

use super::WorkQueryService;
use crate::{
    db_pool::{build_pool, TEST_POOL_MAX_SIZE},
    test_support::{unique_id, uuid_ish},
    work_store::{NewWorkTask, WorkStore},
};

// Separate publication and page reads called one task both claims-only and native.
// Retaining one snapshot must keep the new publication out until the next request.
#[tokio::test]
async fn concurrent_native_publication_cannot_mix_claims_only_metadata_with_new_tasks() {
    let Ok(url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let pool = build_pool(&url, TEST_POOL_MAX_SIZE).expect("build writer pool");
    let writer = WorkStore::connect(&pool).await.expect("connect writer");
    let tenant = unique_id("publication-snapshot");
    let repository = "repository";
    let now = SystemTime::now();
    let mut task = NewWorkTask {
        tenant_id: tenant.clone(),
        repository_id: repository.to_owned(),
        task_id: "existing".to_owned(),
        title: "Existing task".to_owned(),
        acceptance: "Keep reads coherent".to_owned(),
        goal_id: None,
        declared_paths: Vec::new(),
        declared_symbols: Vec::new(),
        published_by: "publisher".to_owned(),
    };
    writer
        .create_task(&task, "existing-event", now)
        .await
        .expect("publish existing task");
    pool.get()
        .await
        .expect("checkout fixture connection")
        .execute(
            "INSERT INTO delegated_claims (tenant_id, repository_id, task_id, owner_id, branch, \
                 claim_started_at, lease_expires_at, claim_lapses, paths, symbols) \
             VALUES ($1,$2,'pending','owner','main',$3,$4,0,'{}','{}')",
            &[
                &tenant,
                &repository,
                &now,
                &(now + Duration::from_secs(600)),
            ],
        )
        .await
        .expect("seed scoped claims-only record");
    task.task_id = "pending".to_owned();
    task.title = "Newly published task".to_owned();

    let read_pool = build_pool(&url, 1).expect("build single-connection reader pool");
    read_pool.resize(1);
    let reader = WorkQueryService::new(
        WorkStore::connect(&read_pool)
            .await
            .expect("connect reader"),
    );
    let connection = read_pool.get().await.expect("checkout reader connection");
    let reader_pid: i32 = connection
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .expect("reader pid")
        .get(0);
    let barrier_key = uuid_ish() as i64;
    connection
        .batch_execute(&format!(
            "CREATE FUNCTION pg_temp.pause_work_publication() RETURNS boolean \
             LANGUAGE plpgsql VOLATILE AS $$ BEGIN \
                 PERFORM pg_advisory_xact_lock({barrier_key}::bigint); \
                 RETURN true; \
             END $$; \
             CREATE TEMP VIEW delegated_claims AS \
                 SELECT * FROM public.delegated_claims \
                 WHERE tenant_id = '{tenant}' AND pg_temp.pause_work_publication()"
        ))
        .await
        .expect("install connection-local publication barrier");
    drop(connection);
    let mut barrier_connection = pool.get().await.expect("checkout barrier connection");
    let barrier = barrier_connection
        .transaction()
        .await
        .expect("begin barrier");
    barrier
        .query_one("SELECT pg_advisory_xact_lock($1)", &[&barrier_key])
        .await
        .expect("hold barrier");
    let observer = pool.get().await.expect("checkout observer");
    let request = v1::ListWorkTasksRequest {
        tenant_id: tenant.clone(),
        repository_id: repository.to_owned(),
        page: 1,
        page_size: 20,
        state: String::new(),
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(20), async {
        tokio::join!(
            reader.list_work_tasks(Request::new(request.clone())),
            async {
                loop {
                    let waiting: bool = observer
                        .query_one(
                            "SELECT EXISTS (SELECT 1 FROM pg_locks \
                         WHERE pid = $1 AND locktype = 'advisory' AND NOT granted)",
                            &[&reader_pid],
                        )
                        .await
                        .expect("observe blocked publication read")
                        .get(0);
                    if waiting {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
                writer
                    .create_task(&task, "new-event", now)
                    .await
                    .expect("commit new native task");
                barrier.commit().await.expect("release reader");
            }
        )
    })
    .await
    .expect("controlled publication race must not hang");
    read_pool
        .get()
        .await
        .expect("checkout cleanup connection")
        .batch_execute(
            "DROP VIEW pg_temp.delegated_claims; DROP FUNCTION pg_temp.pause_work_publication()",
        )
        .await
        .expect("remove connection-local barrier");

    let result = result.expect("read page snapshot").into_inner();
    let publication = result.publication.expect("publication metadata");
    eprintln!(
        "publication snapshot: total={}, claims_only={}",
        result.total, publication.claims_only_total
    );
    assert_eq!(publication.state, "current");
    assert_eq!(publication.claims_only_total, 1);
    assert_eq!(publication.claims_only[0].task_id, "pending");
    assert_eq!(
        result.total, 1,
        "a task still claims-only in the snapshot cannot appear in that page"
    );
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].task_id, "existing");

    let latest = reader
        .list_work_tasks(Request::new(request))
        .await
        .expect("read fresh snapshot")
        .into_inner();
    assert_eq!(latest.total, 2);
    assert_eq!(latest.items.len(), 2);
    let publication = latest.publication.expect("fresh publication metadata");
    assert_eq!(publication.claims_only_total, 0);
    assert!(publication.claims_only.is_empty());
}

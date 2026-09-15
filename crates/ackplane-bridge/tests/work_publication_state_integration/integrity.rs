use std::time::{Duration, SystemTime};

use ackplane_server::{
    db_pool::{build_pool, TEST_POOL_MAX_SIZE},
    work_store::{NewWorkTask, WorkStore, WorkStoreError},
};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::json;
use tower::ServiceExt;

use super::{application, body_json, enroll_repository, unique_id};

/// Missing provenance must not look current or disappear behind pagination;
/// refuse corrupt reads, diagnose the task, and leave its stored evidence intact.
#[tokio::test]
async fn missing_source_position_refuses_pages_and_detail_without_repair_or_scope_leaks() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let unique = unique_id("http-work-integrity");
    let tenant_id = format!("tenant-{unique}");
    let repository_id = format!("repository-{unique}");
    let sibling_repository = format!("sibling-{unique}");
    let foreign_tenant_id = format!("foreign-tenant-{unique}");
    let foreign_repository = format!("foreign-{unique}");
    let task_id = format!("shared-task-{unique}");
    let valid_task_id = format!("valid-task-{unique}");
    for (tenant, repository) in [
        (&tenant_id, &repository_id),
        (&tenant_id, &sibling_repository),
        (&foreign_tenant_id, &repository_id),
        (&foreign_tenant_id, &foreign_repository),
    ] {
        let enrollment_id = unique_id("http-integrity-enrollment");
        enroll_repository(&database_url, tenant, repository, &enrollment_id).await;
    }
    let pool = build_pool(&database_url, TEST_POOL_MAX_SIZE).expect("build Work fixture pool");
    let writer = WorkStore::connect(&pool)
        .await
        .expect("connect Work writer");
    for (tenant, repository, task, title) in [
        (&tenant_id, &repository_id, &task_id, "Corrupt target"),
        (
            &tenant_id,
            &repository_id,
            &valid_task_id,
            "Healthy neighbor",
        ),
        (
            &tenant_id,
            &sibling_repository,
            &task_id,
            "Sibling repository task",
        ),
        (
            &foreign_tenant_id,
            &repository_id,
            &task_id,
            "Other tenant task",
        ),
        (
            &foreign_tenant_id,
            &foreign_repository,
            &task_id,
            "Foreign repository task",
        ),
    ] {
        let event_id = unique_id("http-integrity-event");
        writer
            .create_task(
                &NewWorkTask {
                    tenant_id: tenant.clone(),
                    repository_id: repository.clone(),
                    task_id: task.clone(),
                    title: title.to_owned(),
                    acceptance: "Preserve scoped publication evidence".to_owned(),
                    goal_id: None,
                    declared_paths: Vec::new(),
                    declared_symbols: Vec::new(),
                    published_by: "http-integrity-publisher".to_owned(),
                },
                &event_id,
                SystemTime::now(),
            )
            .await
            .expect("publish scoped native task");
    }
    let app = application(&database_url, &tenant_id, &pool).await;
    let foreign_app = application(&database_url, &foreign_tenant_id, &pool).await;
    for (query, total) in [("?state=completed", 0), ("?page=99&page_size=1", 2)] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repositories/{repository_id}/work{query}"))
                    .body(Body::empty())
                    .expect("build healthy empty page request"),
            )
            .await
            .expect("serve healthy empty page");
        assert_eq!(response.status(), StatusCode::OK);
        let page = body_json(response).await;
        assert_eq!(page["items"], json!([]));
        assert_eq!(page["total"], total);
        assert_eq!(
            page["publication"],
            json!({
                "state": "current", "claims_only_total": 0, "claims_only": [],
            })
        );
    }

    let fixture = pool
        .get()
        .await
        .expect("checkout corruption fixture connection");
    assert_eq!(
        fixture
            .execute(
                "UPDATE work_tasks SET source_event_position = NULL \
         WHERE tenant_id = $1 AND repository_id = $2 AND task_id = $3",
                &[&tenant_id, &repository_id, &task_id],
            )
            .await
            .expect("remove only the owned task source position"),
        1
    );
    let snapshot_sql = "SELECT jsonb_build_object( \
        'tasks', (SELECT jsonb_agg(to_jsonb(task) ORDER BY tenant_id, repository_id, task_id) \
                  FROM work_tasks task WHERE tenant_id = ANY($1)), \
        'history', (SELECT jsonb_agg(to_jsonb(history) \
                    ORDER BY tenant_id, repository_id, task_id, stream_position) \
                    FROM work_task_history history WHERE tenant_id = ANY($1)))::text";
    let tenants = vec![tenant_id.clone(), foreign_tenant_id.clone()];
    let before: String = fixture
        .query_one(snapshot_sql, &[&tenants])
        .await
        .expect("snapshot corrupted source and history")
        .get(0);

    for uri in [
        format!("/api/v1/repositories/{repository_id}/work"),
        format!("/api/v1/repositories/{repository_id}/work?state=completed"),
        format!("/api/v1/repositories/{repository_id}/work?page=99&page_size=1"),
        format!("/api/v1/repositories/{repository_id}/work/{task_id}"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(&uri)
                    .body(Body::empty())
                    .expect("build corrupt read"),
            )
            .await
            .expect("serve corrupt read");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{uri}");
        assert!(
            to_bytes(response.into_body(), 1_048_576)
                .await
                .expect("read refusal body")
                .is_empty(),
            "task data escaped through {uri}"
        );
    }
    let doctor = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/repositories/{repository_id}/work/doctor"))
                .body(Body::empty())
                .expect("build diagnostic request"),
        )
        .await
        .expect("serve diagnostic request");
    assert_eq!(doctor.status(), StatusCode::OK);
    assert_eq!(
        body_json(doctor).await["findings"],
        json!([{
            "kind": "inconsistent_projection",
            "task_id": task_id,
            "detail": "source event position is missing",
            "related_task_id": null,
            "title": null,
            "goal_id": null,
            "path": null,
            "wait_id": null,
            "question": null,
            "owner_id": null,
            "since_seconds": null,
        }])
    );

    for (router, repository, task, title) in [
        (&app, &repository_id, &valid_task_id, "Healthy neighbor"),
        (
            &app,
            &sibling_repository,
            &task_id,
            "Sibling repository task",
        ),
        (&foreign_app, &repository_id, &task_id, "Other tenant task"),
        (
            &foreign_app,
            &foreign_repository,
            &task_id,
            "Foreign repository task",
        ),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/repositories/{repository}/work/{task}"))
                    .body(Body::empty())
                    .expect("build scoped healthy detail"),
            )
            .await
            .expect("serve scoped healthy detail");
        assert_eq!(response.status(), StatusCode::OK);
        let detail = body_json(response).await;
        assert_eq!(detail["task"]["task_id"], json!(task));
        assert_eq!(detail["task"]["title"], title);
        assert_eq!(
            detail["history"].as_array().expect("history array").len(),
            1
        );
        assert_eq!(detail["history"][0]["to_state"], "open");
    }
    for suffix in [String::new(), format!("/{task_id}"), "/doctor".to_owned()] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/v1/repositories/{foreign_repository}/work{suffix}"
                    ))
                    .body(Body::empty())
                    .expect("build foreign repository request"),
            )
            .await
            .expect("serve foreign repository request");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(to_bytes(response.into_body(), 1_048_576)
            .await
            .expect("read foreign refusal body")
            .is_empty());
    }
    let after: String = fixture
        .query_one(snapshot_sql, &[&tenants])
        .await
        .expect("recheck source and history after HTTP reads")
        .get(0);
    assert_eq!(
        after, before,
        "read resources must not repair source positions or history"
    );
}

/// A Work outage must return 503 for owned resources without revealing foreign
/// repositories; visibility checks must use Fleet before touching Work storage.
#[tokio::test]
async fn work_storage_failures_return_503_only_after_repository_visibility() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let unique = unique_id("http-work-storage");
    let tenant_id = format!("tenant-{unique}");
    let repository_id = format!("repository-{unique}");
    let foreign_tenant_id = format!("foreign-tenant-{unique}");
    let foreign_repository = format!("foreign-{unique}");
    let unknown_repository = format!("unknown-{unique}");
    enroll_repository(&database_url, &tenant_id, &repository_id, &unique).await;
    enroll_repository(
        &database_url,
        &foreign_tenant_id,
        &foreign_repository,
        &format!("foreign-{unique}"),
    )
    .await;
    let reader_pool = build_pool(&database_url, 1).expect("build single-connection Work pool");
    reader_pool.resize(1);
    let writer = WorkStore::connect(&reader_pool)
        .await
        .expect("connect fixture writer");
    writer
        .create_task(
            &NewWorkTask {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                task_id: "owned-task".to_owned(),
                title: "Owned task during a Work outage".to_owned(),
                acceptance: "Preserve visibility checks".to_owned(),
                goal_id: None,
                declared_paths: Vec::new(),
                declared_symbols: Vec::new(),
                published_by: "http-storage-publisher".to_owned(),
            },
            "owned-event",
            SystemTime::now(),
        )
        .await
        .expect("publish owned task before the storage outage");
    let app = application(&database_url, &tenant_id, &reader_pool).await;

    for close_pool in [false, true] {
        if close_pool {
            reader_pool.close();
        } else {
            reader_pool
                .get()
                .await
                .expect("checkout query-shape fixture")
                .batch_execute("CREATE TEMP VIEW work_tasks AS SELECT 1 AS invalid_shape")
                .await
                .expect("shadow Work tasks only on the reader connection");
        }
        let mut responses = Vec::new();
        for (repository, expected) in [
            (&repository_id, StatusCode::SERVICE_UNAVAILABLE),
            (&foreign_repository, StatusCode::NOT_FOUND),
            (&unknown_repository, StatusCode::NOT_FOUND),
        ] {
            for suffix in ["", "/owned-task", "/doctor"] {
                let uri = format!("/api/v1/repositories/{repository}/work{suffix}");
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .uri(&uri)
                            .body(Body::empty())
                            .expect("build unavailable Work request"),
                    )
                    .await
                    .expect("serve unavailable Work request");
                responses.push((uri, expected, response));
            }
        }
        if !close_pool {
            reader_pool
                .get()
                .await
                .expect("checkout query-shape cleanup")
                .batch_execute("DROP VIEW pg_temp.work_tasks")
                .await
                .expect("remove connection-local invalid view before assertions");
        }
        for (uri, expected, response) in responses {
            assert_eq!(
                response.status(),
                expected,
                "{uri}, closed Work pool: {close_pool}"
            );
            assert!(
                to_bytes(response.into_body(), 1_048_576)
                    .await
                    .expect("read unavailable response body")
                    .is_empty(),
                "task data escaped through {uri}, closed Work pool: {close_pool}"
            );
        }
    }
}

/// Separate page and publication reads could call one task both native and
/// claims-only; a blocked HTTP read must retain its snapshot across a native commit.
#[tokio::test]
async fn concurrent_native_publication_keeps_http_page_and_claims_in_one_snapshot() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        eprintln!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let unique = unique_id("http-work-snapshot");
    let tenant_id = format!("tenant-{unique}");
    let repository_id = format!("repository-{unique}");
    enroll_repository(&database_url, &tenant_id, &repository_id, &unique).await;
    let pool = build_pool(&database_url, TEST_POOL_MAX_SIZE).expect("build writer pool");
    let writer = WorkStore::connect(&pool)
        .await
        .expect("connect native task writer");
    let now = SystemTime::now();
    let mut task = NewWorkTask {
        tenant_id: tenant_id.clone(),
        repository_id: repository_id.clone(),
        task_id: "existing".to_owned(),
        title: "Existing native task".to_owned(),
        acceptance: "Keep the HTTP page and publication coherent".to_owned(),
        goal_id: None,
        declared_paths: Vec::new(),
        declared_symbols: Vec::new(),
        published_by: "http-snapshot-publisher".to_owned(),
    };
    writer
        .create_task(&task, "existing-event", now)
        .await
        .expect("publish existing task");
    let observer = pool
        .get()
        .await
        .expect("checkout fixture and lock observer");
    observer
        .execute(
            "INSERT INTO delegated_claims (tenant_id, repository_id, task_id, owner_id, branch, \
             claim_started_at, lease_expires_at, claim_lapses, paths, symbols) \
         VALUES ($1,$2,'pending','owner','main',$3,$4,0,'{}','{}')",
            &[
                &tenant_id,
                &repository_id,
                &now,
                &(now + Duration::from_secs(600)),
            ],
        )
        .await
        .expect("seed scoped live claims-only task");
    task.task_id = "pending".to_owned();
    task.title = "Newly published native task".to_owned();

    let reader_pool = build_pool(&database_url, 1).expect("build single-connection reader pool");
    reader_pool.resize(1);
    let app = application(&database_url, &tenant_id, &reader_pool).await;
    let connection = reader_pool.get().await.expect("checkout reader connection");
    let reader_pid: i32 = connection
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .expect("read reader backend pid")
        .get(0);
    let mut barrier_bytes = [0_u8; 8];
    getrandom::getrandom(&mut barrier_bytes).expect("generate isolated barrier key");
    let barrier_key = i64::from_le_bytes(barrier_bytes) & i64::MAX;
    connection
        .batch_execute(&format!(
            "CREATE FUNCTION pg_temp.pause_work_publication() RETURNS boolean \
         LANGUAGE plpgsql VOLATILE AS $$ BEGIN \
             PERFORM pg_advisory_xact_lock({barrier_key}::bigint); \
             RETURN true; \
         END $$; \
         CREATE TEMP VIEW delegated_claims AS \
             SELECT * FROM public.delegated_claims \
             WHERE tenant_id = '{tenant_id}' AND pg_temp.pause_work_publication()"
        ))
        .await
        .expect("install connection-local publication barrier");
    drop(connection);
    let mut barrier_connection = pool.get().await.expect("checkout barrier connection");
    let barrier = barrier_connection
        .transaction()
        .await
        .expect("begin barrier transaction");
    barrier
        .query_one("SELECT pg_advisory_xact_lock($1)", &[&barrier_key])
        .await
        .expect("hold publication barrier");
    let uri = format!("/api/v1/repositories/{repository_id}/work?page=1&page_size=20");
    let request = Request::builder()
        .uri(&uri)
        .body(Body::empty())
        .expect("build publication snapshot request");
    let controlled = tokio::time::timeout(Duration::from_secs(20), async {
        tokio::join!(app.clone().oneshot(request), async {
            loop {
                let waiting: bool = observer
                    .query_one(
                        "SELECT EXISTS (SELECT 1 FROM pg_locks \
                     WHERE pid = $1 AND locktype = 'advisory' AND NOT granted)",
                        &[&reader_pid],
                    )
                    .await?
                    .get(0);
                if waiting {
                    break;
                }
                tokio::task::yield_now().await;
            }
            writer.create_task(&task, "pending-event", now).await?;
            barrier.commit().await?;
            Ok::<(), WorkStoreError>(())
        })
    })
    .await;
    reader_pool
        .get()
        .await
        .expect("checkout publication barrier cleanup")
        .batch_execute(
            "DROP VIEW pg_temp.delegated_claims; DROP FUNCTION pg_temp.pause_work_publication()",
        )
        .await
        .expect("remove connection-local barrier before assertions");

    let (response, publication) =
        controlled.expect("controlled HTTP publication race must not hang");
    publication.expect("commit native publication and release reader");
    let response = response.expect("serve publication snapshot request");
    assert_eq!(response.status(), StatusCode::OK);
    let snapshot = body_json(response).await;
    assert_eq!(snapshot["total"], 1);
    assert_eq!(snapshot["items"].as_array().expect("task array").len(), 1);
    assert_eq!(snapshot["items"][0]["task_id"], "existing");
    assert_eq!(snapshot["publication"]["state"], "current");
    assert_eq!(snapshot["publication"]["claims_only_total"], 1);
    assert_eq!(
        snapshot["publication"]["claims_only"]
            .as_array()
            .expect("claims array")
            .len(),
        1
    );
    assert_eq!(
        snapshot["publication"]["claims_only"][0]["task_id"],
        "pending"
    );

    let latest = app
        .oneshot(
            Request::builder()
                .uri(&uri)
                .body(Body::empty())
                .expect("build fresh snapshot request"),
        )
        .await
        .expect("serve fresh snapshot request");
    assert_eq!(latest.status(), StatusCode::OK);
    let latest = body_json(latest).await;
    assert_eq!(latest["total"], 2);
    assert_eq!(
        latest["items"].as_array().expect("fresh task array").len(),
        2
    );
    assert_eq!(
        latest["publication"],
        json!({
            "state": "current", "claims_only_total": 0, "claims_only": [],
        })
    );
}

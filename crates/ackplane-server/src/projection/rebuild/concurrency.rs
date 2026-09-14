use std::time::Duration;

use crate::{
    db_pool::{build_pool, TEST_POOL_MAX_SIZE},
    ledger::{DedupKey, LedgerStore},
    projection::{
        tests::{require_test_database, structural_fact_envelope},
        Projector, StructuralFact, UnembeddedNode,
    },
    test_support::{unique_id, uuid_ish},
};

// A background scan can become stale before its queued rebuild starts. Replaying
// an already-current repository must not delete vectors another worker just stored.
#[tokio::test]
async fn a_delayed_stale_scan_does_not_discard_newly_indexed_vectors() {
    let url = require_test_database!();
    let pool = build_pool(&url, TEST_POOL_MAX_SIZE).unwrap();
    let ledger = LedgerStore::connect(&pool).await.unwrap();
    let writer = Projector::connect(&pool).await.unwrap();
    let tenant = unique_id("delayed-projection-scan");
    let repository = "repository";
    let fact = StructuralFact {
        node_id: "artifact:source.rs".into(),
        node_type: "artifact".into(),
        label: "current label".into(),
        edges: vec![],
    };
    ledger
        .append(&structural_fact_envelope(
            DedupKey {
                tenant_id: tenant.clone(),
                repository_id: repository.into(),
                producer_id: "producer".into(),
                producer_sequence: 1,
            },
            b"delayed-scan-source",
            &fact,
        ))
        .await
        .unwrap();

    let scan_pool = build_pool(&url, 1).unwrap();
    scan_pool.resize(1);
    let worker = Projector::connect(&scan_pool).await.unwrap();
    let scan_connection = scan_pool.get().await.unwrap();
    let reader_pid: i32 = scan_connection
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .unwrap()
        .get(0);
    let barrier_key = uuid_ish() as i64;
    scan_connection
        .batch_execute(&format!(
            "CREATE FUNCTION pg_temp.pause_projection_scan() RETURNS boolean \
             LANGUAGE plpgsql VOLATILE AS $$ BEGIN \
                 PERFORM pg_advisory_xact_lock({barrier_key}::bigint); \
                 RETURN true; \
             END $$; \
             CREATE TEMP VIEW ledger_records AS \
                 SELECT * FROM public.ledger_records \
                 WHERE tenant_id = '{tenant}' AND repository_id = '{repository}' \
                   AND pg_temp.pause_projection_scan()"
        ))
        .await
        .unwrap();
    drop(scan_connection);

    let mut barrier_connection = pool.get().await.unwrap();
    let barrier = barrier_connection.transaction().await.unwrap();
    barrier
        .query_one("SELECT pg_advisory_xact_lock($1)", &[&barrier_key])
        .await
        .unwrap();
    let observer = pool.get().await.unwrap();
    let vector = vec![0.25_f32; 768];
    let (rebuilt, caught_up) = tokio::time::timeout(Duration::from_secs(20), async {
        tokio::join!(worker.rebuild_stale(), async {
            loop {
                let waiting: bool = observer
                    .query_one(
                        "SELECT EXISTS (SELECT 1 FROM pg_locks \
                         WHERE pid = $1 AND locktype = 'advisory' AND NOT granted)",
                        &[&reader_pid],
                    )
                    .await
                    .unwrap()
                    .get(0);
                if waiting {
                    break;
                }
                tokio::task::yield_now().await;
            }
            let summary = writer.rebuild(&tenant, repository).await.unwrap();
            assert_eq!(summary.stream_position, 1);
            assert!(writer
                .upsert_embedding(
                    &tenant,
                    repository,
                    &UnembeddedNode {
                        node_id: fact.node_id.clone(),
                        label: fact.label.clone(),
                    },
                    "model",
                    &vector,
                )
                .await
                .unwrap());
            let freshness = writer.freshness(&tenant, repository).await.unwrap().unwrap();
            eprintln!(
                "stale scan backend {reader_pid} blocked; another worker caught up to {} and indexed before release",
                freshness.stream_position
            );
            barrier.commit().await.unwrap();
            freshness
        })
    })
    .await
    .expect("the controlled stale scan must not hang");

    let scan_connection = scan_pool.get().await.unwrap();
    scan_connection
        .batch_execute(
            "DROP VIEW pg_temp.ledger_records; DROP FUNCTION pg_temp.pause_projection_scan()",
        )
        .await
        .unwrap();
    drop(scan_connection);
    let stored = observer
        .query_opt(
            "SELECT embedding FROM projected_node_embeddings \
             WHERE tenant_id = $1 AND repository_id = $2 AND node_id = $3 AND model = 'model'",
            &[&tenant, &repository, &fact.node_id],
        )
        .await
        .unwrap();
    assert!(
        stored.is_some(),
        "the delayed scan discarded a newly indexed vector"
    );
    let stored: pgvector::Vector = stored.unwrap().get(0);
    assert_eq!(stored.as_slice(), vector.as_slice());
    assert_eq!(
        rebuilt.unwrap(),
        0,
        "a stale scan is not authority to rebuild again"
    );
    assert_eq!(
        writer.freshness(&tenant, repository).await.unwrap(),
        Some(caught_up)
    );
}

#[tokio::test]
async fn rebuild_lock_serializes_one_repository_without_blocking_another() {
    let url = require_test_database!();
    let pool = build_pool(&url, TEST_POOL_MAX_SIZE).unwrap();
    let ledger = LedgerStore::connect(&pool).await.unwrap();
    let writer = Projector::connect(&pool).await.unwrap();
    let tenant = unique_id("projection-lock-scope");
    let source = StructuralFact {
        node_id: "artifact:source.rs".into(),
        node_type: "artifact".into(),
        label: "source".into(),
        edges: vec![],
    };
    for repository in ["held", "independent"] {
        ledger
            .append(&structural_fact_envelope(
                DedupKey {
                    tenant_id: tenant.clone(),
                    repository_id: repository.into(),
                    producer_id: "producer".into(),
                    producer_sequence: 1,
                },
                b"repository-lock-source",
                &source,
            ))
            .await
            .unwrap();
    }
    let worker_pool = build_pool(&url, 1).unwrap();
    worker_pool.resize(1);
    let worker = Projector::connect(&worker_pool).await.unwrap();
    let connection = worker_pool.get().await.unwrap();
    let worker_pid: i32 = connection
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .unwrap()
        .get(0);
    drop(connection);

    let mut barrier_connection = pool.get().await.unwrap();
    let barrier = barrier_connection.transaction().await.unwrap();
    barrier
        .query_one(
            "SELECT pg_advisory_xact_lock(hashtext($1), hashtext($2))",
            &[&format!("mindleak.projection:{tenant}"), &"held"],
        )
        .await
        .unwrap();
    let before = writer.freshness(&tenant, "held").await.unwrap();
    let observer = pool.get().await.unwrap();
    let (held, independent) = tokio::time::timeout(Duration::from_secs(20), async {
        tokio::join!(worker.rebuild(&tenant, "held"), async {
            loop {
                let waiting: bool = observer
                    .query_one(
                        "SELECT EXISTS (SELECT 1 FROM pg_locks \
                         WHERE pid = $1 AND locktype = 'advisory' AND NOT granted)",
                        &[&worker_pid],
                    )
                    .await
                    .unwrap()
                    .get(0);
                if waiting {
                    break;
                }
                tokio::task::yield_now().await;
            }
            let independent = writer.rebuild(&tenant, "independent").await.unwrap();
            assert_eq!(independent.nodes, 1);
            assert_eq!(writer.freshness(&tenant, "held").await.unwrap(), before);
            barrier.commit().await.unwrap();
            independent
        })
    })
    .await
    .expect("an unrelated repository must progress while the first is locked");
    assert_eq!(held.unwrap(), independent);

    assert!(writer
        .upsert_embedding(
            &tenant,
            "held",
            &UnembeddedNode {
                node_id: source.node_id,
                label: source.label
            },
            "model",
            &vec![0.25; 768],
        )
        .await
        .unwrap());
    assert_eq!(writer.rebuild(&tenant, "held").await.unwrap(), independent);
    let remaining: i64 = observer
        .query_one(
            "SELECT count(*) FROM projected_node_embeddings WHERE tenant_id = $1 AND repository_id = 'held'",
            &[&tenant],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(
        remaining, 0,
        "explicit rebuild still invalidates derived vectors"
    );
}

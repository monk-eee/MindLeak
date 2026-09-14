use std::time::Duration;

use super::{
    tests::{require_test_database, round_trip_embedding, structural_fact_envelope},
    Projector, StructuralFact, UnembeddedNode,
};
use crate::{
    db_pool::{build_pool, TEST_POOL_MAX_SIZE},
    ledger::{DedupKey, LedgerStore},
    test_support::unique_id,
};

// A raw schema-fixture INSERT can see a deleted tuple while an atomic rebuild
// replaces the same key, then fail its FK check after waiting for that rebuild.
#[tokio::test]
async fn embedding_writers_handle_an_atomic_node_replacement_without_foreign_key_errors() {
    let url = require_test_database!();
    let pool = build_pool(&url, TEST_POOL_MAX_SIZE).unwrap();
    let ledger = LedgerStore::connect(&pool).await.unwrap();
    let projector = Projector::connect(&pool).await.unwrap();
    for schema_fixture in [true, false] {
        let tenant = unique_id("embedding-schema-replacement");
        let repository = "repo-a";
        let fact = StructuralFact {
            node_id: "artifact:src/lib.rs".into(),
            node_type: "artifact".into(),
            label: "src/lib.rs".into(),
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
                b"embedding-schema-replacement",
                &fact,
            ))
            .await
            .unwrap();
        projector.rebuild(&tenant, repository).await.unwrap();

        let writer_pool = build_pool(&url, 1).unwrap();
        writer_pool.resize(1);
        let writer = Projector::connect(&writer_pool).await.unwrap();
        let connection = writer_pool.get().await.unwrap();
        let writer_pid: i32 = connection
            .query_one("SELECT pg_backend_pid()", &[])
            .await
            .unwrap()
            .get(0);
        drop(connection);

        let mut replacement_connection = pool.get().await.unwrap();
        let replacement = replacement_connection.transaction().await.unwrap();
        let replacement_pid: i32 = replacement
            .query_one(
                "SELECT pg_backend_pid(), pg_advisory_xact_lock(hashtext($1), hashtext($2))",
                &[&format!("mindleak.projection:{tenant}"), &repository],
            )
            .await
            .unwrap()
            .get(0);
        replacement
            .execute(
                "DELETE FROM projected_nodes WHERE tenant_id = $1 AND repository_id = $2",
                &[&tenant, &repository],
            )
            .await
            .unwrap();
        replacement
            .execute(
                "INSERT INTO projected_nodes \
             (tenant_id, repository_id, node_id, node_type, label, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, now(), now())",
                &[
                    &tenant,
                    &repository,
                    &fact.node_id,
                    &fact.node_type,
                    &fact.label,
                ],
            )
            .await
            .unwrap();
        let observer = pool.get().await.unwrap();
        let (stored, ()) = tokio::time::timeout(Duration::from_secs(20), async {
            tokio::join!(
                async {
                    if schema_fixture {
                        let stored = round_trip_embedding(&writer, &tenant, repository).await?;
                        assert_eq!(stored.as_slice(), vec![0.1_f32; 768].as_slice());
                        Ok(true)
                    } else {
                        writer
                            .upsert_embedding(
                                &tenant,
                                repository,
                                &UnembeddedNode {
                                    node_id: fact.node_id.clone(),
                                    label: fact.label.clone(),
                                },
                                "nomic-embed-text",
                                &vec![0.1_f32; 768],
                            )
                            .await
                    }
                },
                async {
                    loop {
                        let waiting: bool = observer
                            .query_one(
                                "SELECT $1 = ANY(pg_blocking_pids($2))",
                                &[&replacement_pid, &writer_pid],
                            )
                            .await
                            .unwrap()
                            .get(0);
                        if waiting {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                    replacement.commit().await.unwrap();
                }
            )
        })
        .await
        .expect("the controlled node replacement must finish without hanging");
        let stored = stored.expect("atomic replacement must not report a foreign-key error");
        assert_eq!(stored, schema_fixture);
        if !schema_fixture {
            let current = writer
                .nodes_missing_embedding(&tenant, repository, "nomic-embed-text", 1)
                .await
                .unwrap();
            assert_eq!(current.len(), 1);
            assert!(writer
                .upsert_embedding(
                    &tenant,
                    repository,
                    &current[0],
                    "nomic-embed-text",
                    &vec![0.1_f32; 768],
                )
                .await
                .unwrap());
        }
    }
}

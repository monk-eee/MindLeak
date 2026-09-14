use super::tests::{require_test_database, structural_fact_envelope};
use super::*;
use crate::ledger::{DedupKey, LedgerStore};
use crate::test_support::unique_id;

#[tokio::test]
async fn recall_distinguishes_projection_lag_model_coverage_and_a_real_no_match() {
    let url = require_test_database!();
    let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE).unwrap();
    let ledger = LedgerStore::connect(&pool).await.unwrap();
    let projector = Projector::connect(&pool).await.unwrap();
    let tenant = unique_id("recall-tenant");
    let repository = "repository";
    let mut query = vec![0.0; 768];
    query[0] = 1.0;
    let empty = projector
        .recall(&tenant, repository, "model", &[], 0.5, 10)
        .await
        .unwrap();
    assert_eq!(empty.state(), RecallState::Empty);
    assert_eq!(empty.projected_position, None);
    assert!(!empty.searched);

    for sequence in 1..=2 {
        let source = StructuralFact {
            node_id: format!("artifact:{sequence}"),
            node_type: "artifact".into(),
            label: format!("source {sequence}"),
            edges: vec![],
        };
        ledger
            .append(&structural_fact_envelope(
                DedupKey {
                    tenant_id: tenant.clone(),
                    repository_id: repository.into(),
                    producer_id: "producer".into(),
                    producer_sequence: sequence,
                },
                format!("digest-{sequence}").as_bytes(),
                &source,
            ))
            .await
            .unwrap();
        let lagging = projector
            .recall(&tenant, repository, "model", &query, 0.5, 10)
            .await
            .unwrap();
        assert_eq!(
            lagging.state(),
            if sequence == 1 {
                RecallState::NotYetProjected
            } else {
                RecallState::Stale
            }
        );
        assert!(!lagging.searched);
        assert!(lagging.nodes.is_empty());
        projector.rebuild(&tenant, repository).await.unwrap();
        let unembedded = projector
            .recall(&tenant, repository, "model", &query, 0.5, 10)
            .await
            .unwrap();
        assert_eq!(unembedded.state(), RecallState::NotYetEmbedded);
        assert_eq!(unembedded.projected_nodes, sequence);
        assert_eq!(unembedded.embedded_nodes, 0);
        assert_eq!(
            unembedded.projected_position,
            Some(unembedded.ledger_position)
        );
        assert!(unembedded.projected_at.is_some());
        assert!(projector
            .upsert_embedding(
                &tenant,
                repository,
                &UnembeddedNode {
                    node_id: source.node_id.clone(),
                    label: source.label.clone()
                },
                "model",
                &query
            )
            .await
            .unwrap());
        let matched = projector
            .recall(&tenant, repository, "model", &query, 0.5, 10)
            .await
            .unwrap();
        assert_eq!(
            matched.state(),
            if sequence == 1 {
                RecallState::Current
            } else {
                RecallState::PartiallyEmbedded
            }
        );
        assert!(matched.searched);
        assert_eq!(matched.nodes.len(), 1);
        assert_eq!(matched.nodes[0].node_id, source.node_id);
        assert!((matched.nodes[0].similarity - 1.0).abs() < 1e-6);
        let other_model = projector
            .recall(&tenant, repository, "other-model", &query, 0.5, 10)
            .await
            .unwrap();
        assert_eq!(other_model.state(), RecallState::NotYetEmbedded);
    }
    let mut orthogonal = vec![0.0; 768];
    orthogonal[1] = 1.0;
    let no_match = projector
        .recall(&tenant, repository, "model", &orthogonal, 0.5, 10)
        .await
        .unwrap();
    assert!(no_match.searched);
    assert!(no_match.nodes.is_empty());
    assert_eq!(no_match.state(), RecallState::PartiallyEmbedded);
}

// Regression: statement snapshots can mix old coverage with rebuilt, empty candidates.
// Block the first read until a rebuild commits; one repeatable snapshot keeps the old hit.
#[tokio::test]
async fn recall_keeps_old_coverage_checkpoint_and_hits_during_concurrent_rebuild() {
    let url = require_test_database!();
    let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE).unwrap();
    let ledger = LedgerStore::connect(&pool).await.unwrap();
    let projector = Projector::connect(&pool).await.unwrap();
    let tenant = unique_id("recall-snapshot-tenant");
    let repository = unique_id("recall-snapshot-repository");
    let old_fact = StructuralFact {
        node_id: "artifact:recall-snapshot".into(),
        node_type: "artifact".into(),
        label: "old".into(),
        edges: vec![],
    };
    let mut query = vec![0.0; 768];
    query[0] = 1.0;
    ledger
        .append(&structural_fact_envelope(
            DedupKey {
                tenant_id: tenant.clone(),
                repository_id: repository.clone(),
                producer_id: "producer".into(),
                producer_sequence: 1,
            },
            b"old-digest",
            &old_fact,
        ))
        .await
        .unwrap();
    let old_projection = projector.rebuild(&tenant, &repository).await.unwrap();
    assert!(projector
        .upsert_embedding(
            &tenant,
            &repository,
            &UnembeddedNode {
                node_id: old_fact.node_id.clone(),
                label: old_fact.label.clone(),
            },
            "model",
            &query,
        )
        .await
        .unwrap());
    let before = projector
        .recall(&tenant, &repository, "model", &query, 0.5, 10)
        .await
        .unwrap();
    assert_eq!(before.state(), RecallState::Current);
    assert!(before.projected_at.is_some());

    let read_pool = crate::db_pool::build_pool(&url, 1).unwrap();
    read_pool.resize(1);
    assert_eq!(read_pool.status().max_size, 1);
    let reader = Projector::connect(&read_pool).await.unwrap();
    let reader_connection = reader.connection().await.unwrap();
    let reader_pid: i32 = reader_connection
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .unwrap()
        .get(0);
    let advisory_key = crate::test_support::uuid_ish() as i64;
    reader_connection
        .batch_execute(&format!(
            "CREATE FUNCTION pg_temp.recall_snapshot_barrier() RETURNS boolean
             LANGUAGE plpgsql VOLATILE AS $barrier$
             BEGIN
                 IF current_setting('transaction_read_only') <> 'on' THEN
                     RAISE EXCEPTION 'recall must use a read-only transaction';
                 END IF;
                 PERFORM pg_advisory_xact_lock({advisory_key}::bigint);
                 RETURN true;
             END;
             $barrier$;
             CREATE TEMP VIEW projection_state AS
                 SELECT * FROM public.projection_state
                 WHERE tenant_id = '{}' AND repository_id = '{}'
                   AND pg_temp.recall_snapshot_barrier();
             SET search_path = pg_temp, public;",
            tenant.replace('\'', "''"),
            repository.replace('\'', "''"),
        ))
        .await
        .unwrap();
    drop(reader_connection);

    let mut writer_connection = projector.connection().await.unwrap();
    let lock_transaction = writer_connection.transaction().await.unwrap();
    lock_transaction
        .query_one("SELECT pg_advisory_xact_lock($1)", &[&advisory_key])
        .await
        .unwrap();
    let (snapshot, new_projection) = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        async {
            tokio::join!(
                reader.recall(&tenant, &repository, "model", &query, 0.5, 10),
                async {
                    loop {
                        let blocked: bool = lock_transaction
                            .query_one(
                                "SELECT EXISTS (
                                     SELECT 1 FROM pg_locks
                                     WHERE pid = $1 AND locktype = 'advisory' AND NOT granted
                                       AND classid::bigint = (($2::bigint >> 32) & 4294967295)
                                       AND objid::bigint = ($2::bigint & 4294967295)
                                       AND objsubid = 1
                                 )",
                                &[&reader_pid, &advisory_key],
                            )
                            .await
                            .unwrap()
                            .get(0);
                        if blocked {
                            break;
                        }
                        tokio::task::yield_now().await;
                    }
                    eprintln!(
                        "read-only recall backend {reader_pid} reached advisory barrier {advisory_key}"
                    );
                    let new_fact = StructuralFact {
                        label: "new".into(),
                        ..old_fact.clone()
                    };
                    ledger
                        .append(&structural_fact_envelope(
                            DedupKey {
                                tenant_id: tenant.clone(),
                                repository_id: repository.clone(),
                                producer_id: "producer".into(),
                                producer_sequence: 2,
                            },
                            b"new-digest",
                            &new_fact,
                        ))
                        .await
                        .unwrap();
                    let rebuilt = projector.rebuild(&tenant, &repository).await.unwrap();
                    assert_eq!(rebuilt.nodes, 1);
                    assert!(rebuilt.stream_position > old_projection.stream_position);
                    eprintln!(
                        "writer committed rebuild at checkpoint {} (was {}) before releasing barrier",
                        rebuilt.stream_position, old_projection.stream_position
                    );
                    lock_transaction.commit().await.unwrap();
                    rebuilt
                }
            )
        },
    )
    .await
    .expect("recall must reach the database barrier and finish after rebuild within 20s");
    let snapshot = snapshot.unwrap();

    let fresh = projector
        .recall(&tenant, &repository, "model", &query, 0.5, 10)
        .await
        .unwrap();
    assert_eq!(fresh.state(), RecallState::NotYetEmbedded);
    assert!(!fresh.searched);
    assert_eq!((fresh.projected_nodes, fresh.embedded_nodes), (1, 0));
    assert_eq!(fresh.ledger_position, new_projection.stream_position);
    assert_eq!(
        fresh.projected_position,
        Some(new_projection.stream_position)
    );
    assert!(fresh.projected_at.is_some());
    assert!(fresh.nodes.is_empty());
    let new_label: String = writer_connection
        .query_one(
            "SELECT label FROM projected_nodes
             WHERE tenant_id = $1 AND repository_id = $2 AND node_id = $3",
            &[&tenant, &repository, &old_fact.node_id],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(new_label, "new");
    eprintln!(
        "fresh writer read: NotYetEmbedded, coverage 1/0, checkpoint {}, label {new_label}",
        fresh.ledger_position
    );

    let cleanup_connection = reader.connection().await.unwrap();
    let cleanup_pid: i32 = cleanup_connection
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(cleanup_pid, reader_pid);
    cleanup_connection
        .batch_execute(
            "DROP VIEW pg_temp.projection_state;
             DROP FUNCTION pg_temp.recall_snapshot_barrier();
             RESET search_path;",
        )
        .await
        .unwrap();
    drop(cleanup_connection);
    drop(reader);
    drop(read_pool);

    assert_eq!(snapshot.state(), RecallState::Current);
    assert!(snapshot.searched);
    assert_eq!((snapshot.projected_nodes, snapshot.embedded_nodes), (1, 1));
    assert_eq!(snapshot.ledger_position, old_projection.stream_position);
    assert_eq!(
        snapshot.projected_position,
        Some(old_projection.stream_position)
    );
    assert_eq!(snapshot.projected_at, before.projected_at);
    assert_eq!(
        snapshot.nodes.len(),
        1,
        "old coverage must retain its old hit"
    );
    assert_eq!(snapshot.nodes[0].node_id, old_fact.node_id);
    assert_eq!(snapshot.nodes[0].label, "old");
    assert!((snapshot.nodes[0].similarity - 1.0).abs() < 1e-6);
}

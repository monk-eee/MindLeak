use super::*;
use crate::ledger::{DedupKey, LedgerStore};
use crate::projection::tests::{require_test_database, structural_fact_envelope};
use crate::test_support::uuid_ish;

#[tokio::test]
async fn a_rebuild_reproduces_the_same_projection_from_the_same_ledger() {
    let url = require_test_database!();
    let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
        .expect("the test database url should build a pool");
    let ledger = LedgerStore::connect(&pool).await.expect("connect ledger");
    let projector = Projector::connect(&pool).await.expect("connect projector");
    let tenant = format!("t-{}", uuid_ish());
    let repo = "repo-a".to_string();

    let file = StructuralFact {
        node_id: "artifact:src/lib.rs".to_string(),
        node_type: "artifact".to_string(),
        label: "src/lib.rs".to_string(),
        edges: vec![StructuralEdgeFact {
            target_id: "symbol:src/lib.rs:main".to_string(),
            relation: "contains".to_string(),
            base_weight: 1.0,
            half_life_hours: 168.0,
        }],
    };
    let symbol = StructuralFact {
        node_id: "symbol:src/lib.rs:main".to_string(),
        node_type: "symbol".to_string(),
        label: "main".to_string(),
        edges: vec![],
    };

    ledger
        .append(&structural_fact_envelope(
            DedupKey {
                tenant_id: tenant.clone(),
                repository_id: repo.clone(),
                producer_id: "producer-a".to_string(),
                producer_sequence: 1,
            },
            b"digest-1",
            &file,
        ))
        .await
        .expect("append file fact");
    ledger
        .append(&structural_fact_envelope(
            DedupKey {
                tenant_id: tenant.clone(),
                repository_id: repo.clone(),
                producer_id: "producer-a".to_string(),
                producer_sequence: 2,
            },
            b"digest-2",
            &symbol,
        ))
        .await
        .expect("append symbol fact");

    let first = projector.rebuild(&tenant, &repo).await.expect("rebuild");
    assert_eq!(
        first,
        ProjectionSummary {
            nodes: 2,
            edges: 1,
            stream_position: 2,
        }
    );

    // Rebuilding again from the same ledger, with nothing appended in
    // between, must reproduce exactly the same projection (ADR-0087
    // clause 1) — this is the rebuild-and-diff test the ADR requires.
    let second = projector
        .rebuild(&tenant, &repo)
        .await
        .expect("rebuild again");
    assert_eq!(second, first);

    let freshness = projector
        .freshness(&tenant, &repo)
        .await
        .expect("freshness")
        .expect("projected at least once");
    assert_eq!(freshness.stream_position, 2);
}

/// Real-database coverage, reproducing the contention `rebuild`'s retry
/// closes: many concurrent rebuilds of *unrelated* tenants used to
/// deadlock under enough parallel load (no FK ties `projected_edges` to
/// `projected_nodes`, so this is B-tree index-page lock contention, not a
/// logical schema bug) — confirmed live once the Coverage CI gate began
/// running these tests against a real Postgres (ADR-0118) instead of
/// hollow-skipping them. Every task must still succeed; a deadlock is
/// retried internally, never surfaced to the caller.
#[tokio::test]
async fn concurrent_rebuilds_of_unrelated_tenants_all_succeed() {
    let url = require_test_database!();
    let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
        .expect("the test database url should build a pool");
    let tasks = (0..12).map(|i| {
        let pool = pool.clone();
        tokio::spawn(async move {
            let ledger = LedgerStore::connect(&pool).await.expect("connect ledger");
            let projector = Projector::connect(&pool).await.expect("connect projector");
            let tenant = format!("t-{}-{}", i, uuid_ish());
            let repo = "repo-a".to_string();

            let fact = StructuralFact {
                node_id: "artifact:src/lib.rs".to_string(),
                node_type: "artifact".to_string(),
                label: "src/lib.rs".to_string(),
                edges: vec![StructuralEdgeFact {
                    target_id: "symbol:src/lib.rs:main".to_string(),
                    relation: "contains".to_string(),
                    base_weight: 1.0,
                    half_life_hours: 168.0,
                }],
            };
            ledger
                .append(&structural_fact_envelope(
                    DedupKey {
                        tenant_id: tenant.clone(),
                        repository_id: repo.clone(),
                        producer_id: "producer-a".to_string(),
                        producer_sequence: 1,
                    },
                    b"digest-1",
                    &fact,
                ))
                .await
                .expect("append fact");

            projector.rebuild(&tenant, &repo).await.expect("rebuild")
        })
    });
    for task in tasks {
        let summary = task.await.expect("task did not panic");
        assert_eq!(summary.nodes, 1);
        assert_eq!(summary.edges, 1);
    }
}

#[tokio::test]
async fn an_unprojected_repository_reports_no_freshness() {
    let url = require_test_database!();
    let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
        .expect("the test database url should build a pool");
    let projector = Projector::connect(&pool).await.expect("connect");
    let tenant = format!("t-{}", uuid_ish());

    let freshness = projector
        .freshness(&tenant, "repo-never-projected")
        .await
        .expect("freshness query");
    assert_eq!(freshness, None);
}

#[tokio::test]
async fn stale_projections_finds_a_repository_ahead_of_its_checkpoint_and_rebuild_stale_catches_it_up(
) {
    let url = require_test_database!();
    let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
        .expect("the test database url should build a pool");
    let ledger = LedgerStore::connect(&pool).await.expect("connect ledger");
    let projector = Projector::connect(&pool).await.expect("connect projector");
    let tenant = format!("t-{}", uuid_ish());
    let repo = "repo-stale".to_string();

    let fact = StructuralFact {
        node_id: "artifact:src/lib.rs".to_string(),
        node_type: "artifact".to_string(),
        label: "src/lib.rs".to_string(),
        edges: vec![],
    };
    ledger
        .append(&structural_fact_envelope(
            DedupKey {
                tenant_id: tenant.clone(),
                repository_id: repo.clone(),
                producer_id: "producer-a".to_string(),
                producer_sequence: 1,
            },
            b"digest-1",
            &fact,
        ))
        .await
        .expect("append fact");

    let stale = projector.stale_projections().await.expect("stale query");
    assert!(stale.contains(&StaleProjection {
        tenant_id: tenant.clone(),
        repository_id: repo.clone(),
    }));

    // `rebuilt` counts every stale repository across every tenant in the
    // shared test database, not just this one (other tests may be
    // running concurrently against it), so only a lower bound on the
    // count is safe to assert here; `freshness` below is the assertion
    // that actually proves THIS repository was rebuilt.
    let rebuilt = projector.rebuild_stale().await.expect("rebuild_stale");
    assert!(
        rebuilt >= 1,
        "expected at least this repository to be rebuilt, got {rebuilt}"
    );

    let freshness = projector
        .freshness(&tenant, &repo)
        .await
        .expect("freshness")
        .expect("projected after rebuild_stale");
    assert_eq!(freshness.stream_position, 1);
}

#[tokio::test]
async fn a_repository_already_caught_up_is_not_reported_stale_or_redundantly_rebuilt() {
    let url = require_test_database!();
    let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
        .expect("the test database url should build a pool");
    let ledger = LedgerStore::connect(&pool).await.expect("connect ledger");
    let projector = Projector::connect(&pool).await.expect("connect projector");
    let tenant = format!("t-{}", uuid_ish());
    let repo = "repo-caught-up".to_string();

    let fact = StructuralFact {
        node_id: "artifact:src/lib.rs".to_string(),
        node_type: "artifact".to_string(),
        label: "src/lib.rs".to_string(),
        edges: vec![],
    };
    ledger
        .append(&structural_fact_envelope(
            DedupKey {
                tenant_id: tenant.clone(),
                repository_id: repo.clone(),
                producer_id: "producer-a".to_string(),
                producer_sequence: 1,
            },
            b"digest-1",
            &fact,
        ))
        .await
        .expect("append fact");

    // Catch it up directly (not through rebuild_stale, which scans every
    // tenant and would make this setup step depend on concurrent test
    // activity in the shared test database).
    projector
        .rebuild(&tenant, &repo)
        .await
        .expect("catch up directly");

    // This proves only that a fresh scan excludes the caught-up repository.
    // The concurrency regression separately covers a scan becoming stale
    // before its queued rebuild starts.
    let stale = projector.stale_projections().await.expect("stale query");
    assert!(!stale.contains(&StaleProjection {
        tenant_id: tenant.clone(),
        repository_id: repo.clone(),
    }));
}

#[tokio::test]
async fn a_repository_with_zero_structural_facts_is_never_marked_projected() {
    let url = require_test_database!();
    let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
        .expect("the test database url should build a pool");
    let projector = Projector::connect(&pool).await.expect("connect projector");
    let tenant = format!("t-{}", uuid_ish());
    let repo = "repo-never-published-a-structural-fact".to_string();

    let stale = projector.stale_projections().await.expect("stale query");
    assert!(!stale
        .iter()
        .any(|repository| repository.tenant_id == tenant && repository.repository_id == repo));

    // A pass may rebuild other tenants' stale repositories concurrently;
    // the count is not asserted here, only that this specific repository
    // stays unprojected afterward.
    projector.rebuild_stale().await.expect("rebuild_stale");

    let freshness = projector
        .freshness(&tenant, &repo)
        .await
        .expect("freshness query");
    assert_eq!(freshness, None);
}

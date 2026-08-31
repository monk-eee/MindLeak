use super::*;

impl Projector {
    /// Drop and replay one repository's projection from its committed
    /// [`STRUCTURAL_FACT_PAYLOAD_TYPE`] ledger records, in stream order, all
    /// inside one transaction — a caller never observes a half-rebuilt
    /// projection.
    ///
    /// Retries a genuine PostgreSQL deadlock (SQLSTATE 40P01) a bounded number
    /// of times. No foreign key ties `projected_edges` to `projected_nodes`
    /// (see `migrations/0002_projection.sql`), so this is B-tree index-page
    /// lock contention between concurrent rebuilds of *unrelated* tenants
    /// under enough parallel load, not a logical schema bug — confirmed live
    /// once the Coverage CI gate started running these tests against a real
    /// Postgres (ADR-0118) instead of hollow-skipping them. PostgreSQL's own
    /// documentation recommends the client simply reissue a deadlocked
    /// transaction; rebuild is naturally idempotent (exactly what
    /// [`a_rebuild_reproduces_the_same_projection_from_the_same_ledger`]
    /// proves), so a clean retry from scratch is safe.
    pub async fn rebuild(
        &self,
        tenant_id: &str,
        repository_id: &str,
    ) -> Result<ProjectionSummary, ProjectionError> {
        const MAX_DEADLOCK_RETRIES: u32 = 3;
        let mut attempt = 0;
        loop {
            match self.rebuild_once(tenant_id, repository_id).await {
                Err(ProjectionError::Database(error))
                    if attempt < MAX_DEADLOCK_RETRIES && error.code().is_some_and(is_deadlock) =>
                {
                    attempt += 1;
                }
                result => return result,
            }
        }
    }

    async fn rebuild_once(
        &self,
        tenant_id: &str,
        repository_id: &str,
    ) -> Result<ProjectionSummary, ProjectionError> {
        let mut connection = self.connection().await?;
        let transaction = connection.transaction().await?;

        transaction
            .execute(
                "DELETE FROM projected_edges WHERE tenant_id = $1 AND repository_id = $2",
                &[&tenant_id, &repository_id],
            )
            .await?;
        transaction
            .execute(
                "DELETE FROM projected_nodes WHERE tenant_id = $1 AND repository_id = $2",
                &[&tenant_id, &repository_id],
            )
            .await?;

        let rows = transaction
            .query(
                "SELECT payload, stream_position FROM ledger_records \
                 WHERE tenant_id = $1 AND repository_id = $2 AND payload_type = $3 \
                 ORDER BY stream_position ASC",
                &[&tenant_id, &repository_id, &STRUCTURAL_FACT_PAYLOAD_TYPE],
            )
            .await?;

        let mut last_position: i64 = 0;
        for row in &rows {
            let payload: Vec<u8> = row.get(0);
            let position: i64 = row.get(1);
            let fact: StructuralFact = serde_json::from_slice(&payload)
                .map_err(|source| ProjectionError::MalformedFact { position, source })?;

            transaction
                .execute(
                    "INSERT INTO projected_nodes \
                         (tenant_id, repository_id, node_id, node_type, label, created_at, updated_at) \
                     VALUES ($1, $2, $3, $4, $5, now(), now()) \
                     ON CONFLICT (tenant_id, repository_id, node_id) DO UPDATE SET \
                         node_type = EXCLUDED.node_type, label = EXCLUDED.label, updated_at = now()",
                    &[
                        &tenant_id,
                        &repository_id,
                        &fact.node_id,
                        &fact.node_type,
                        &fact.label,
                    ],
                )
                .await?;

            for edge in &fact.edges {
                transaction
                    .execute(
                        "INSERT INTO projected_edges \
                             (tenant_id, repository_id, source_id, target_id, relation, \
                              base_weight, half_life_hours, updated_at) \
                         VALUES ($1, $2, $3, $4, $5, $6, $7, now()) \
                         ON CONFLICT (tenant_id, repository_id, source_id, target_id, relation) \
                         DO UPDATE SET \
                             base_weight = EXCLUDED.base_weight, \
                             half_life_hours = EXCLUDED.half_life_hours, \
                             updated_at = now()",
                        &[
                            &tenant_id,
                            &repository_id,
                            &fact.node_id,
                            &edge.target_id,
                            &edge.relation,
                            &edge.base_weight,
                            &edge.half_life_hours,
                        ],
                    )
                    .await?;
            }

            last_position = position;
        }

        transaction
            .execute(
                "INSERT INTO projection_state (tenant_id, repository_id, stream_position, projected_at) \
                 VALUES ($1, $2, $3, now()) \
                 ON CONFLICT (tenant_id, repository_id) DO UPDATE SET \
                     stream_position = EXCLUDED.stream_position, projected_at = now()",
                &[&tenant_id, &repository_id, &last_position],
            )
            .await?;

        let node_count: i64 = transaction
            .query_one(
                "SELECT count(*) FROM projected_nodes WHERE tenant_id = $1 AND repository_id = $2",
                &[&tenant_id, &repository_id],
            )
            .await?
            .get(0);
        let edge_count: i64 = transaction
            .query_one(
                "SELECT count(*) FROM projected_edges WHERE tenant_id = $1 AND repository_id = $2",
                &[&tenant_id, &repository_id],
            )
            .await?
            .get(0);

        transaction.commit().await?;
        Ok(ProjectionSummary {
            nodes: node_count,
            edges: edge_count,
            stream_position: last_position,
        })
    }

    /// Every repository whose committed structural facts are ahead of its
    /// projection checkpoint (ADR-0086 clause 9): a missing `projection_state`
    /// row reads as checkpoint zero, so a repository that has never been
    /// projected but has at least one structural fact is included too. A
    /// repository with zero structural-fact records never appears here —
    /// there is nothing for `rebuild` to give it.
    async fn stale_projections(&self) -> Result<Vec<StaleProjection>, ProjectionError> {
        let connection = self.connection().await?;
        let rows = connection
            .query(
                "SELECT lr.tenant_id, lr.repository_id \
                 FROM ledger_records lr \
                 LEFT JOIN projection_state ps \
                    ON ps.tenant_id = lr.tenant_id AND ps.repository_id = lr.repository_id \
                 WHERE lr.payload_type = $1 \
                 GROUP BY lr.tenant_id, lr.repository_id, ps.stream_position \
                 HAVING max(lr.stream_position) > COALESCE(ps.stream_position, 0)",
                &[&STRUCTURAL_FACT_PAYLOAD_TYPE],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| StaleProjection {
                tenant_id: row.get(0),
                repository_id: row.get(1),
            })
            .collect())
    }

    /// One polling pass (ADR-0086 clause 9): rebuild every repository
    /// [`stale_projections`](Self::stale_projections) finds. One repository's
    /// rebuild failing is logged and does not stop the rest, or the caller's
    /// next tick — a projection worker's job is to catch a stream back up,
    /// not to guarantee every tick succeeds. Returns how many repositories
    /// were actually rebuilt.
    pub async fn rebuild_stale(&self) -> Result<usize, ProjectionError> {
        let stale = self.stale_projections().await?;
        let mut rebuilt = 0;
        for repository in &stale {
            match self
                .rebuild(&repository.tenant_id, &repository.repository_id)
                .await
            {
                Ok(summary) => {
                    tracing::info!(
                        tenant_id = %repository.tenant_id,
                        repository_id = %repository.repository_id,
                        nodes = summary.nodes,
                        edges = summary.edges,
                        stream_position = summary.stream_position,
                        "rebuilt a repository's graph projection"
                    );
                    rebuilt += 1;
                }
                Err(error) => {
                    tracing::error!(
                        tenant_id = %repository.tenant_id,
                        repository_id = %repository.repository_id,
                        %error,
                        "projection rebuild failed for one repository; continuing with the rest"
                    );
                }
            }
        }
        Ok(rebuilt)
    }

    /// This repository's projection freshness, or `None` if it has never
    /// been projected (ADR-0087 clause 10).
    pub async fn freshness(
        &self,
        tenant_id: &str,
        repository_id: &str,
    ) -> Result<Option<ProjectionFreshness>, ProjectionError> {
        let connection = self.connection().await?;
        let row = connection
            .query_opt(
                "SELECT stream_position, projected_at FROM projection_state \
                 WHERE tenant_id = $1 AND repository_id = $2",
                &[&tenant_id, &repository_id],
            )
            .await?;
        Ok(row.map(|row| ProjectionFreshness {
            stream_position: row.get(0),
            projected_at: row.get(1),
        }))
    }
}

/// Run [`Projector::rebuild_stale`] on `interval` forever (ADR-0086 clause 9:
/// "Projection workers read the durable ledger through checkpoints").
/// Intended to run as its own background task (`tokio::spawn`) alongside the
/// gRPC server; a tick's database error is logged and the loop keeps polling
/// rather than exiting, since a missed tick is simply caught up by the next
/// one, not a fatal condition for the worker.
pub async fn run_projection_worker(projector: Projector, interval: std::time::Duration) {
    let mut ticker = tokio::time::interval(interval);
    loop {
        ticker.tick().await;
        if let Err(error) = projector.rebuild_stale().await {
            tracing::error!(%error, "could not check for stale projections this tick");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{DedupKey, LedgerStore};
    use crate::projection::tests::{require_test_database, structural_fact_envelope};
    use crate::test_support::uuid_ish;

    #[tokio::test]
    async fn a_rebuild_reproduces_the_same_projection_from_the_same_ledger() {
        let url = require_test_database!();
        let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
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
            let url = url.clone();
            let pool = pool.clone();
            tokio::spawn(async move {
                let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
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
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
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
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
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

        // Nothing new has been appended, so this repository must no longer
        // be reported as stale. `rebuild_stale` only ever rebuilds what this
        // query returns (it is a plain for-loop over it), so excluding this
        // repository here is exactly what proves it can never be redundantly
        // rebuilt -- a second, separate timing-based check would only repeat
        // the same guarantee less reliably under concurrent test load.
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
}

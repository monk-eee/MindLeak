//! Project the Ackplane graph from ledger records (ADR-0087 clauses 1, 2, 3,
//! 6, 10).
//!
//! Every table here is defined in `migrations/0002_projection.sql`, applied
//! idempotently by [`Projector::connect`]. [`Projector::rebuild`] is the only
//! writer: it drops a repository's projected nodes and edges and replays its
//! committed [`STRUCTURAL_FACT_PAYLOAD_TYPE`] ledger records in stream order,
//! so a rebuild always reproduces the same projection from the same ledger
//! (clause 1) — proven by [`a_rebuild_reproduces_the_same_projection_from_the_same_ledger`].
//!
//! [`Projector::bounded_neighborhood`] is the read side: an ordinary
//! recursive-CTE traversal (clause 2, no extension required) that honours the
//! same bounded, best-first contract `mindleak-core`'s `GraphStore` already
//! implements — seed set, max depth, max admitted nodes, per-node fanout
//! limited to the strongest edges, dangling edges dropped (clause 3) — with
//! effective weight computed in the query, never stored (clause 6). It also
//! returns the projection's freshness (clause 10): the ledger stream position
//! folded in and when that rebuild ran, so a stale or unbuilt projection is
//! legible rather than presented as current fact.
//!
//! # Testing without a database (ADR-0088 clause 2)
//!
//! As in [`crate::ledger`], the tests that exercise a real database are
//! opt-in via `ACKPLANE_TEST_DATABASE_URL` and skip (rather than fail) when it
//! is absent, so `cargo test --workspace` keeps passing with no PostgreSQL,
//! Docker, or network reachable.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_postgres::{Client, NoTls};

const MIGRATION: &str = include_str!("../migrations/0002_projection.sql");

/// The `payload_type` a ledger record must carry to be folded into the graph
/// projection. Any other payload type is ignored by [`Projector::rebuild`]
/// rather than rejected — the ledger accepts every accepted record type, and
/// deciding which ones are graph-relevant is the projector's job, not the
/// ledger's.
pub const STRUCTURAL_FACT_PAYLOAD_TYPE: &str = "structural_fact";

/// One edge a [`StructuralFact`] declares from its node.
///
/// Deliberately a plain, JSON-encoded storage-layer type, not the generated
/// Protobuf message: the wire contract for what a repository node actually
/// publishes belongs to ADR-0083's gRPC handler task, which can define and
/// change it independently of how the projector folds an accepted payload
/// into these tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuralEdgeFact {
    pub target_id: String,
    pub relation: String,
    pub base_weight: f64,
    pub half_life_hours: f64,
}

/// A minimised structural summary (ADR-0087 clause 7): one node and the
/// edges it declares, published under [`STRUCTURAL_FACT_PAYLOAD_TYPE`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuralFact {
    pub node_id: String,
    pub node_type: String,
    pub label: String,
    #[serde(default)]
    pub edges: Vec<StructuralEdgeFact>,
}

/// Row counts left in a repository's projection after [`Projector::rebuild`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectionSummary {
    pub nodes: i64,
    pub edges: i64,
    pub stream_position: i64,
}

/// A projected node returned by [`Projector::bounded_neighborhood`], with the
/// traversal depth it was admitted at.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedNode {
    pub node_id: String,
    pub node_type: String,
    pub label: String,
    pub depth: i32,
}

/// A projected edge returned by [`Projector::bounded_neighborhood`]. Effective
/// weight is intentionally absent: it is a function of `now`, so a caller
/// recomputes it from `base_weight`/`half_life_hours`/`updated_at` rather
/// than treating a returned number as durable.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedEdge {
    pub source_id: String,
    pub target_id: String,
    pub relation: String,
    pub base_weight: f64,
    pub half_life_hours: f64,
    pub updated_at: SystemTime,
}

/// A repository's projection freshness (ADR-0087 clause 10): `None` means the
/// repository has never been projected, not position zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectionFreshness {
    pub stream_position: i64,
    pub projected_at: SystemTime,
}

/// A bounded neighbourhood around a seed set, carrying the freshness of the
/// projection it was read from.
#[derive(Debug, Clone, PartialEq)]
pub struct BoundedNeighborhood {
    pub nodes: Vec<ProjectedNode>,
    pub edges: Vec<ProjectedEdge>,
    pub freshness: Option<ProjectionFreshness>,
}

/// One repository whose committed structural facts are ahead of its
/// projection checkpoint (ADR-0086 clause 9): either it has never been
/// projected, or the ledger has moved past the position it was last
/// projected at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleProjection {
    pub tenant_id: String,
    pub repository_id: String,
}

#[derive(Debug, Error)]
pub enum ProjectionError {
    #[error("structural fact at stream position {position} could not be decoded: {source}")]
    MalformedFact {
        position: i64,
        #[source]
        source: serde_json::Error,
    },
    #[error("projection database error: {0}")]
    Database(#[from] tokio_postgres::Error),
}

/// Whether a SQLSTATE is PostgreSQL's own deadlock detection (40P01) — the
/// one server-reported error its own documentation recommends retrying
/// rather than treating as permanent.
fn is_deadlock(code: &tokio_postgres::error::SqlState) -> bool {
    *code == tokio_postgres::error::SqlState::T_R_DEADLOCK_DETECTED
}

/// A connection to Ackplane's projection tables (ADR-0087).
pub struct Projector {
    client: Client,
}

impl Projector {
    /// Connect and apply the projection schema. Every statement in the
    /// migration is idempotent, matching [`crate::ledger::LedgerStore::connect`].
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (mut client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane projection connection closed with an error");
            }
        });
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::PROJECTION,
            MIGRATION,
        )
        .await?;
        Ok(Self { client })
    }

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
        &mut self,
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
        &mut self,
        tenant_id: &str,
        repository_id: &str,
    ) -> Result<ProjectionSummary, ProjectionError> {
        let transaction = self.client.transaction().await?;

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
    async fn stale_projections(&self) -> Result<Vec<StaleProjection>, tokio_postgres::Error> {
        let rows = self
            .client
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
    pub async fn rebuild_stale(&mut self) -> Result<usize, ProjectionError> {
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
    ) -> Result<Option<ProjectionFreshness>, tokio_postgres::Error> {
        let row = self
            .client
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

    /// A bounded, relevance-first neighbourhood around `seeds` (ADR-0087
    /// clause 3, mirroring `GraphStore::bounded_neighborhood`): ordinary SQL,
    /// a recursive CTE with explicit depth and per-node fanout bounds, dangling
    /// edges dropped, and effective weight computed in the query rather than
    /// stored (clause 6).
    pub async fn bounded_neighborhood(
        &self,
        tenant_id: &str,
        repository_id: &str,
        seeds: &[String],
        max_depth: i32,
        max_nodes: i32,
        max_fanout: i32,
    ) -> Result<BoundedNeighborhood, ProjectionError> {
        let freshness = self.freshness(tenant_id, repository_id).await?;

        let frontier_rows = self
            .client
            .query(
                "WITH RECURSIVE frontier(node_id, depth) AS ( \
                     SELECT node_id, 0 \
                     FROM projected_nodes \
                     WHERE tenant_id = $1 AND repository_id = $2 AND node_id = ANY($3) \
                     UNION \
                     SELECT next.neighbor_id, frontier.depth + 1 \
                     FROM frontier \
                     JOIN LATERAL ( \
                         SELECT \
                             CASE WHEN e.source_id = frontier.node_id THEN e.target_id \
                                  ELSE e.source_id END AS neighbor_id, \
                             e.base_weight * power( \
                                 2.0, \
                                 -(extract(epoch FROM (now() - e.updated_at)) / 3600.0) \
                                     / e.half_life_hours \
                             ) AS effective_weight \
                         FROM projected_edges e \
                         WHERE e.tenant_id = $1 AND e.repository_id = $2 \
                           AND (e.source_id = frontier.node_id OR e.target_id = frontier.node_id) \
                         ORDER BY effective_weight DESC \
                         LIMIT $4 \
                     ) next ON TRUE \
                     WHERE frontier.depth < $5 \
                 ) \
                 SELECT node_id, min(depth) AS depth \
                 FROM frontier \
                 GROUP BY node_id \
                 ORDER BY depth ASC, node_id ASC \
                 LIMIT $6",
                &[
                    &tenant_id,
                    &repository_id,
                    &seeds,
                    &(max_fanout as i64),
                    &max_depth,
                    &(max_nodes as i64),
                ],
            )
            .await?;

        let mut admitted_ids: Vec<String> = Vec::with_capacity(frontier_rows.len());
        let mut depth_by_id = std::collections::HashMap::with_capacity(frontier_rows.len());
        for row in &frontier_rows {
            let node_id: String = row.get(0);
            let depth: i32 = row.get(1);
            depth_by_id.insert(node_id.clone(), depth);
            admitted_ids.push(node_id);
        }

        if admitted_ids.is_empty() {
            return Ok(BoundedNeighborhood {
                nodes: Vec::new(),
                edges: Vec::new(),
                freshness,
            });
        }

        let node_rows = self
            .client
            .query(
                "SELECT node_id, node_type, label FROM projected_nodes \
                 WHERE tenant_id = $1 AND repository_id = $2 AND node_id = ANY($3)",
                &[&tenant_id, &repository_id, &admitted_ids],
            )
            .await?;
        let nodes = node_rows
            .into_iter()
            .map(|row| {
                let node_id: String = row.get(0);
                let depth = depth_by_id.get(&node_id).copied().unwrap_or(0);
                ProjectedNode {
                    node_id,
                    node_type: row.get(1),
                    label: row.get(2),
                    depth,
                }
            })
            .collect();

        // Dangling edges dropped: only edges whose endpoints are both admitted
        // are returned (ADR-0087 clause 3).
        let edge_rows = self
            .client
            .query(
                "SELECT source_id, target_id, relation, base_weight, half_life_hours, updated_at \
                 FROM projected_edges \
                 WHERE tenant_id = $1 AND repository_id = $2 \
                   AND source_id = ANY($3) AND target_id = ANY($3)",
                &[&tenant_id, &repository_id, &admitted_ids],
            )
            .await?;
        let edges = edge_rows
            .into_iter()
            .map(|row| ProjectedEdge {
                source_id: row.get(0),
                target_id: row.get(1),
                relation: row.get(2),
                base_weight: row.get(3),
                half_life_hours: row.get(4),
                updated_at: row.get(5),
            })
            .collect();

        Ok(BoundedNeighborhood {
            nodes,
            edges,
            freshness,
        })
    }

    /// A default seed set for a repository's Context Graph view when the
    /// caller has not chosen one yet: the most recently touched nodes, most
    /// recent first, bounded by `limit`. Read-only and independent of
    /// `bounded_neighborhood` -- it never traverses edges, only samples
    /// `projected_nodes` directly, so it stays cheap even on a repository
    /// with no useful edge structure yet.
    pub async fn sample_nodes(
        &self,
        tenant_id: &str,
        repository_id: &str,
        limit: i64,
    ) -> Result<Vec<ProjectedNode>, tokio_postgres::Error> {
        let rows = self
            .client
            .query(
                "SELECT node_id, node_type, label FROM projected_nodes \
                 WHERE tenant_id = $1 AND repository_id = $2 \
                 ORDER BY updated_at DESC, node_id ASC \
                 LIMIT $3",
                &[&tenant_id, &repository_id, &limit],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| ProjectedNode {
                node_id: row.get(0),
                node_type: row.get(1),
                label: row.get(2),
                depth: 0,
            })
            .collect())
    }
}

/// Run [`Projector::rebuild_stale`] on `interval` forever (ADR-0086 clause 9:
/// "Projection workers read the durable ledger through checkpoints").
/// Intended to run as its own background task (`tokio::spawn`) alongside the
/// gRPC server; a tick's database error is logged and the loop keeps polling
/// rather than exiting, since a missed tick is simply caught up by the next
/// one, not a fatal condition for the worker.
pub async fn run_projection_worker(mut projector: Projector, interval: std::time::Duration) {
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
    use crate::ledger::{DedupKey, EventEnvelope, LedgerStore, ProvenanceClass};
    use crate::test_support::uuid_ish;

    fn structural_fact_envelope(
        key: DedupKey,
        digest: &[u8],
        fact: &StructuralFact,
    ) -> EventEnvelope {
        EventEnvelope {
            key,
            payload: serde_json::to_vec(fact).expect("encode structural fact"),
            payload_digest: digest.to_vec(),
            schema_version: "v1".to_string(),
            occurred_at: std::time::SystemTime::now(),
            payload_type: STRUCTURAL_FACT_PAYLOAD_TYPE.to_string(),
            previous_envelope_digest: None,
            signing_key_id: None,
            signature: None,
            provenance: ProvenanceClass::EnrolledNode,
        }
    }

    #[test]
    fn a_structural_fact_round_trips_through_json() {
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
        let encoded = serde_json::to_vec(&fact).unwrap();
        let decoded: StructuralFact = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, fact);
    }

    #[test]
    fn a_fact_with_no_edges_key_omitted_still_decodes() {
        let decoded: StructuralFact =
            serde_json::from_str(r#"{"node_id":"artifact:a","node_type":"artifact","label":"a"}"#)
                .unwrap();
        assert_eq!(decoded.edges, Vec::<StructuralEdgeFact>::new());
    }

    /// Real-database coverage. Opt-in via `ACKPLANE_TEST_DATABASE_URL`, and
    /// skipped (not failed) when it is unset (ADR-0088 clause 2).
    macro_rules! require_test_database {
        () => {
            match std::env::var("ACKPLANE_TEST_DATABASE_URL") {
                Ok(url) => url,
                Err(_) => {
                    eprintln!(
                        "skipping: ACKPLANE_TEST_DATABASE_URL is not set (ADR-0088 clause 2 keeps \
                         this opt-in rather than requiring PostgreSQL in default CI)"
                    );
                    return;
                }
            }
        };
    }

    #[tokio::test]
    async fn a_rebuild_reproduces_the_same_projection_from_the_same_ledger() {
        let url = require_test_database!();
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
        let mut projector = Projector::connect(&url).await.expect("connect projector");
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

    #[test]
    fn is_deadlock_recognizes_only_the_deadlock_sqlstate() {
        assert!(is_deadlock(
            &tokio_postgres::error::SqlState::T_R_DEADLOCK_DETECTED
        ));
        assert!(!is_deadlock(
            &tokio_postgres::error::SqlState::UNIQUE_VIOLATION
        ));
        assert!(!is_deadlock(
            &tokio_postgres::error::SqlState::T_R_SERIALIZATION_FAILURE
        ));
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
        let tasks = (0..12).map(|i| {
            let url = url.clone();
            tokio::spawn(async move {
                let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
                let mut projector = Projector::connect(&url).await.expect("connect projector");
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
        let projector = Projector::connect(&url).await.expect("connect");
        let tenant = format!("t-{}", uuid_ish());

        let freshness = projector
            .freshness(&tenant, "repo-never-projected")
            .await
            .expect("freshness query");
        assert_eq!(freshness, None);
    }

    #[tokio::test]
    async fn bounded_neighborhood_admits_only_seeds_reachable_within_max_depth() {
        let url = require_test_database!();
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
        let projector = Projector::connect(&url).await.expect("connect projector");
        let mut rebuilder = Projector::connect(&url).await.expect("connect rebuilder");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-chain".to_string();

        // a -> b -> c, a chain three hops long.
        let facts = [
            StructuralFact {
                node_id: "artifact:a".to_string(),
                node_type: "artifact".to_string(),
                label: "a".to_string(),
                edges: vec![StructuralEdgeFact {
                    target_id: "artifact:b".to_string(),
                    relation: "imports".to_string(),
                    base_weight: 1.0,
                    half_life_hours: 168.0,
                }],
            },
            StructuralFact {
                node_id: "artifact:b".to_string(),
                node_type: "artifact".to_string(),
                label: "b".to_string(),
                edges: vec![StructuralEdgeFact {
                    target_id: "artifact:c".to_string(),
                    relation: "imports".to_string(),
                    base_weight: 1.0,
                    half_life_hours: 168.0,
                }],
            },
            StructuralFact {
                node_id: "artifact:c".to_string(),
                node_type: "artifact".to_string(),
                label: "c".to_string(),
                edges: vec![],
            },
        ];
        for (index, fact) in facts.iter().enumerate() {
            ledger
                .append(&structural_fact_envelope(
                    DedupKey {
                        tenant_id: tenant.clone(),
                        repository_id: repo.clone(),
                        producer_id: "producer-a".to_string(),
                        producer_sequence: index as i64 + 1,
                    },
                    format!("digest-{index}").as_bytes(),
                    fact,
                ))
                .await
                .expect("append fact");
        }
        rebuilder.rebuild(&tenant, &repo).await.expect("rebuild");

        let neighborhood = projector
            .bounded_neighborhood(&tenant, &repo, &["artifact:a".to_string()], 1, 10, 10)
            .await
            .expect("bounded neighborhood");

        let mut node_ids: Vec<&str> = neighborhood
            .nodes
            .iter()
            .map(|node| node.node_id.as_str())
            .collect();
        node_ids.sort_unstable();
        assert_eq!(node_ids, vec!["artifact:a", "artifact:b"]);
        assert!(neighborhood.freshness.is_some());

        // c is two hops from the seed and outside max_depth = 1, so the a-b
        // edge is kept (both endpoints admitted) and b-c is dropped as
        // dangling (ADR-0087 clause 3).
        assert_eq!(neighborhood.edges.len(), 1);
        assert_eq!(neighborhood.edges[0].source_id, "artifact:a");
        assert_eq!(neighborhood.edges[0].target_id, "artifact:b");
        // The Bridge Context Graph view recomputes effective weight itself
        // (it is a function of `now`), so a real, non-default timestamp
        // must round-trip through Postgres rather than reading as the Rust
        // zero value.
        assert!(
            neighborhood.edges[0]
                .updated_at
                .duration_since(std::time::UNIX_EPOCH)
                .expect("updated_at is after the epoch")
                .as_secs()
                > 0
        );
    }

    #[tokio::test]
    async fn sample_nodes_bounds_the_result_to_the_requested_limit() {
        let url = require_test_database!();
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
        let mut projector = Projector::connect(&url).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-sample".to_string();

        for (index, node_id) in ["artifact:a", "artifact:b", "artifact:c"]
            .iter()
            .enumerate()
        {
            ledger
                .append(&structural_fact_envelope(
                    DedupKey {
                        tenant_id: tenant.clone(),
                        repository_id: repo.clone(),
                        producer_id: "producer-a".to_string(),
                        producer_sequence: index as i64 + 1,
                    },
                    format!("digest-{index}").as_bytes(),
                    &StructuralFact {
                        node_id: (*node_id).to_string(),
                        node_type: "artifact".to_string(),
                        label: (*node_id).to_string(),
                        edges: vec![],
                    },
                ))
                .await
                .expect("append fact");
        }
        projector.rebuild(&tenant, &repo).await.expect("rebuild");

        let sample = projector
            .sample_nodes(&tenant, &repo, 2)
            .await
            .expect("sample nodes");
        assert_eq!(sample.len(), 2);
    }

    #[tokio::test]
    async fn sample_nodes_orders_deterministically_when_updated_at_ties() {
        let url = require_test_database!();
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
        let mut projector = Projector::connect(&url).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-sample-order".to_string();

        // One `rebuild` call sets every admitted node's `updated_at` to the
        // SAME transaction timestamp, so with no ordering signal left, the
        // node id tiebreaker is what a caller actually observes.
        for (index, node_id) in ["artifact:z", "artifact:a", "artifact:m"]
            .iter()
            .enumerate()
        {
            ledger
                .append(&structural_fact_envelope(
                    DedupKey {
                        tenant_id: tenant.clone(),
                        repository_id: repo.clone(),
                        producer_id: "producer-a".to_string(),
                        producer_sequence: index as i64 + 1,
                    },
                    format!("digest-{index}").as_bytes(),
                    &StructuralFact {
                        node_id: (*node_id).to_string(),
                        node_type: "artifact".to_string(),
                        label: (*node_id).to_string(),
                        edges: vec![],
                    },
                ))
                .await
                .expect("append fact");
        }
        projector.rebuild(&tenant, &repo).await.expect("rebuild");

        let sample = projector
            .sample_nodes(&tenant, &repo, 10)
            .await
            .expect("sample nodes");
        let node_ids: Vec<&str> = sample.iter().map(|node| node.node_id.as_str()).collect();
        assert_eq!(node_ids, vec!["artifact:a", "artifact:m", "artifact:z"]);
    }

    #[tokio::test]
    async fn sample_nodes_is_tenant_scoped() {
        let url = require_test_database!();
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
        let mut projector = Projector::connect(&url).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-sample-tenant".to_string();

        ledger
            .append(&structural_fact_envelope(
                DedupKey {
                    tenant_id: tenant.clone(),
                    repository_id: repo.clone(),
                    producer_id: "producer-a".to_string(),
                    producer_sequence: 1,
                },
                b"digest-1",
                &StructuralFact {
                    node_id: "artifact:a".to_string(),
                    node_type: "artifact".to_string(),
                    label: "a".to_string(),
                    edges: vec![],
                },
            ))
            .await
            .expect("append fact");
        projector.rebuild(&tenant, &repo).await.expect("rebuild");

        let other_tenant = format!("{tenant}-other");
        let sample = projector
            .sample_nodes(&other_tenant, &repo, 10)
            .await
            .expect("sample nodes for a different tenant");
        assert!(sample.is_empty());
    }

    #[tokio::test]
    async fn stale_projections_finds_a_repository_ahead_of_its_checkpoint_and_rebuild_stale_catches_it_up(
    ) {
        let url = require_test_database!();
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
        let mut projector = Projector::connect(&url).await.expect("connect projector");
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
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
        let mut projector = Projector::connect(&url).await.expect("connect projector");
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
        let mut projector = Projector::connect(&url).await.expect("connect projector");
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

use super::*;

impl Projector {
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
        let connection = self.connection().await?;

        let frontier_rows = connection
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

        let node_rows = connection
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
        let edge_rows = connection
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
    ) -> Result<Vec<ProjectedNode>, ProjectionError> {
        let connection = self.connection().await?;
        let rows = connection
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::DedupKey;
    use crate::ledger::LedgerStore;
    use crate::projection::tests::{require_test_database, structural_fact_envelope};
    use crate::test_support::uuid_ish;

    #[tokio::test]
    async fn bounded_neighborhood_admits_only_seeds_reachable_within_max_depth() {
        let url = require_test_database!();
        let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        let rebuilder = Projector::connect(&pool).await.expect("connect rebuilder");
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
        let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
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
        let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
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
        let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
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
}

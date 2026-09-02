use pgvector::Vector;

use super::*;

/// A projected node with no embedding for `model` yet (ADR-0140 decision 2).
#[derive(Debug, Clone, PartialEq)]
pub struct UnembeddedNode {
    pub node_id: String,
    pub label: String,
}

/// One candidate from the ranking pipeline's first stage (ADR-0140
/// decision 3), carrying the distance PostgreSQL itself measured.
#[derive(Debug, Clone, PartialEq)]
pub struct SimilarNode {
    pub node_id: String,
    pub label: String,
    pub node_type: String,
    /// pgvector's `<=>` cosine distance: `0.0` is identical, `1.0`
    /// orthogonal, `2.0` opposite. Cosine *similarity* -- the raw score
    /// ADR-0140 decision 4 requires a caller be shown -- is `1.0 - this`.
    /// Reported as measured rather than pre-converted so stage two ranks
    /// against PostgreSQL's own number, not a transformation of it.
    pub cosine_distance: f64,
}

impl Projector {
    /// Nodes in this repository's projection that have no
    /// `projected_node_embeddings` row for `model` yet -- the same
    /// offline-pass candidate query `mindleak_core::embed::
    /// nodes_missing_embeddings` already runs locally against `nodes`,
    /// applied here to the ledger-derived projection instead. Never a second
    /// writer: this only reads `projected_nodes`, so a node reported here is
    /// always one the ledger replay already produced.
    pub async fn nodes_missing_embedding(
        &self,
        tenant_id: &str,
        repository_id: &str,
        model: &str,
        limit: i64,
    ) -> Result<Vec<UnembeddedNode>, ProjectionError> {
        let connection = self.connection().await?;
        let rows = connection
            .query(
                "SELECT n.node_id, n.label FROM projected_nodes n \
                 WHERE n.tenant_id = $1 AND n.repository_id = $2 \
                   AND NOT EXISTS ( \
                       SELECT 1 FROM projected_node_embeddings e \
                       WHERE e.tenant_id = n.tenant_id AND e.repository_id = n.repository_id \
                         AND e.node_id = n.node_id AND e.model = $3 \
                   ) \
                 ORDER BY n.updated_at DESC \
                 LIMIT $4",
                &[&tenant_id, &repository_id, &model, &limit],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| UnembeddedNode {
                node_id: row.get(0),
                label: row.get(1),
            })
            .collect())
    }

    /// Store (or replace) `node_id`'s embedding under `model` (ADR-0140
    /// decision 2).
    ///
    /// The foreign key on `projected_node_embeddings` is load-bearing here,
    /// the same way it already is for `mindleak_core::embed::ensure_table`'s
    /// local, SQLite-backed table: inserting for a node absent from
    /// `projected_nodes` fails with a foreign-key violation rather than
    /// silently creating an embedding describing nothing. This method never
    /// invents a node the ledger replay did not produce -- a caller must have
    /// found `node_id` via [`Self::nodes_missing_embedding`] or otherwise
    /// confirmed it is already projected.
    pub async fn upsert_embedding(
        &self,
        tenant_id: &str,
        repository_id: &str,
        node_id: &str,
        model: &str,
        embedding: &[f32],
    ) -> Result<(), ProjectionError> {
        let vector = Vector::from(embedding.to_vec());
        let connection = self.connection().await?;
        connection
            .execute(
                "INSERT INTO projected_node_embeddings \
                 (tenant_id, repository_id, node_id, model, embedding, updated_at) \
                 VALUES ($1, $2, $3, $4, $5, now()) \
                 ON CONFLICT (tenant_id, repository_id, node_id, model) \
                 DO UPDATE SET embedding = excluded.embedding, updated_at = excluded.updated_at",
                &[&tenant_id, &repository_id, &node_id, &model, &vector],
            )
            .await?;
        Ok(())
    }

    /// Stage one of the ranking pipeline (ADR-0140 decision 3): a bounded
    /// candidate set ordered by pgvector's `<=>` cosine distance, computed
    /// entirely inside PostgreSQL rather than pulled into application memory
    /// for a cosine loop over every stored vector -- the exact scaling limit
    /// `0007_knowledge.sql`'s own comment already names.
    ///
    /// This is candidate *retrieval*, not the answer. `kind_prior`,
    /// `distinctive_cut` and the floor still decide what -- if anything -- is
    /// reported, and they run over this set in application code. Returning a
    /// candidate here does not mean it is worth showing anyone.
    ///
    /// A `model` nothing was embedded under yields an empty candidate set
    /// rather than an error (ADR-0140 decision 1), which is what lets a
    /// caller fall back to the recency/decay path (ADR-0080) instead of
    /// failing. An empty result is therefore genuinely ambiguous between
    /// "nothing is similar" and "nothing is embedded under this model";
    /// [`Self::nodes_missing_embedding`] is what distinguishes them.
    pub async fn similar_nodes(
        &self,
        tenant_id: &str,
        repository_id: &str,
        model: &str,
        query: &[f32],
        limit: i64,
    ) -> Result<Vec<SimilarNode>, ProjectionError> {
        let query = Vector::from(query.to_vec());
        let connection = self.connection().await?;
        let rows = connection
            .query(
                "SELECT n.node_id, n.label, n.node_type, e.embedding <=> $4 AS cosine_distance \
                 FROM projected_node_embeddings e \
                 JOIN projected_nodes n \
                   ON n.tenant_id = e.tenant_id AND n.repository_id = e.repository_id \
                      AND n.node_id = e.node_id \
                 WHERE e.tenant_id = $1 AND e.repository_id = $2 AND e.model = $3 \
                 ORDER BY e.embedding <=> $4 \
                 LIMIT $5",
                &[&tenant_id, &repository_id, &model, &query, &limit],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| SimilarNode {
                node_id: row.get(0),
                label: row.get(1),
                node_type: row.get(2),
                cosine_distance: row.get(3),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{DedupKey, LedgerStore};
    use crate::projection::tests::{require_test_database, structural_fact_envelope};
    use crate::test_support::uuid_ish;

    async fn projected_node(
        projector: &Projector,
        ledger: &LedgerStore,
        tenant: &str,
        repo: &str,
        node_id: &str,
    ) {
        projected_nodes(projector, ledger, tenant, repo, &[node_id]).await;
    }

    /// Rebuild is a drop-and-replay, and `projected_node_embeddings` cascades
    /// on delete -- so every node a test needs must be projected in ONE
    /// rebuild, before any embedding is written, or the embeddings vanish.
    async fn projected_nodes(
        projector: &Projector,
        ledger: &LedgerStore,
        tenant: &str,
        repo: &str,
        node_ids: &[&str],
    ) {
        for (index, node_id) in node_ids.iter().enumerate() {
            let fact = StructuralFact {
                node_id: (*node_id).to_string(),
                node_type: "artifact".to_string(),
                label: (*node_id).to_string(),
                edges: vec![],
            };
            let digest = format!("digest-{index}");
            ledger
                .append(&structural_fact_envelope(
                    DedupKey {
                        tenant_id: tenant.to_string(),
                        repository_id: repo.to_string(),
                        producer_id: "producer-a".to_string(),
                        producer_sequence: index as i64 + 1,
                    },
                    digest.as_bytes(),
                    &fact,
                ))
                .await
                .expect("append structural fact");
        }
        projector
            .rebuild(tenant, repo)
            .await
            .expect("rebuild projects the node");
    }

    /// A 768-dimension vector with the given dimensions set and the rest
    /// zero, so cosine distances between the fixtures are exactly known.
    fn sparse_unit(dimensions: &[(usize, f32)]) -> Vec<f32> {
        let mut vector = vec![0.0_f32; 768];
        for (index, weight) in dimensions {
            vector[*index] = *weight;
        }
        vector
    }

    #[tokio::test]
    async fn a_projected_node_with_no_embedding_is_reported_missing() {
        let url = require_test_database!();
        let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let ledger = LedgerStore::connect(&pool).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-a".to_string();

        projected_node(&projector, &ledger, &tenant, &repo, "artifact:src/lib.rs").await;

        let missing = projector
            .nodes_missing_embedding(&tenant, &repo, "nomic-embed-text", 10)
            .await
            .expect("query succeeds");
        assert_eq!(
            missing,
            vec![UnembeddedNode {
                node_id: "artifact:src/lib.rs".to_string(),
                label: "artifact:src/lib.rs".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn a_node_stops_being_reported_missing_once_embedded() {
        let url = require_test_database!();
        let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let ledger = LedgerStore::connect(&pool).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-a".to_string();

        projected_node(&projector, &ledger, &tenant, &repo, "artifact:src/lib.rs").await;
        projector
            .upsert_embedding(
                &tenant,
                &repo,
                "artifact:src/lib.rs",
                "nomic-embed-text",
                &vec![0.1_f32; 768],
            )
            .await
            .expect("embedding is accepted");

        let missing = projector
            .nodes_missing_embedding(&tenant, &repo, "nomic-embed-text", 10)
            .await
            .expect("query succeeds");
        assert!(missing.is_empty(), "got: {missing:?}");
    }

    /// A node embedded under one model is still missing under a different
    /// one -- re-embedding under a new model must not read as already done.
    #[tokio::test]
    async fn missing_is_scoped_per_model() {
        let url = require_test_database!();
        let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let ledger = LedgerStore::connect(&pool).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-a".to_string();

        projected_node(&projector, &ledger, &tenant, &repo, "artifact:src/lib.rs").await;
        projector
            .upsert_embedding(
                &tenant,
                &repo,
                "artifact:src/lib.rs",
                "nomic-embed-text",
                &vec![0.1_f32; 768],
            )
            .await
            .expect("embedding is accepted");

        let missing = projector
            .nodes_missing_embedding(&tenant, &repo, "a-different-model", 10)
            .await
            .expect("query succeeds");
        assert_eq!(missing.len(), 1, "got: {missing:?}");
    }

    /// Upserting again for the same node/model replaces the vector rather
    /// than erroring or duplicating the row.
    #[tokio::test]
    async fn upserting_the_same_node_and_model_again_replaces_the_embedding() {
        let url = require_test_database!();
        let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let ledger = LedgerStore::connect(&pool).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-a".to_string();

        projected_node(&projector, &ledger, &tenant, &repo, "artifact:src/lib.rs").await;
        projector
            .upsert_embedding(
                &tenant,
                &repo,
                "artifact:src/lib.rs",
                "nomic-embed-text",
                &vec![0.1_f32; 768],
            )
            .await
            .expect("first embedding is accepted");
        projector
            .upsert_embedding(
                &tenant,
                &repo,
                "artifact:src/lib.rs",
                "nomic-embed-text",
                &vec![0.2_f32; 768],
            )
            .await
            .expect("second embedding replaces the first");

        let row = projector
            .connection()
            .await
            .expect("checkout connection to read back the embedding")
            .query_one(
                "SELECT embedding FROM projected_node_embeddings \
                 WHERE tenant_id = $1 AND repository_id = $2 AND node_id = $3 AND model = $4",
                &[&tenant, &repo, &"artifact:src/lib.rs", &"nomic-embed-text"],
            )
            .await
            .expect("exactly one row exists");
        let stored: Vector = row.get(0);
        assert_eq!(stored.as_slice(), vec![0.2_f32; 768].as_slice());
    }

    /// A node absent from the projection cannot be given an embedding --
    /// the foreign key refuses it rather than silently creating one
    /// describing nothing.
    #[tokio::test]
    async fn an_embedding_for_an_unprojected_node_is_refused() {
        let url = require_test_database!();
        let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-a".to_string();

        let error = projector
            .upsert_embedding(
                &tenant,
                &repo,
                "artifact:never-projected.rs",
                "nomic-embed-text",
                &vec![0.1_f32; 768],
            )
            .await
            .expect_err("a node absent from the projection is refused");
        let ProjectionError::Database(database_error) = &error else {
            panic!("expected a database error, got: {error}");
        };
        assert_eq!(
            database_error.code(),
            Some(&tokio_postgres::error::SqlState::FOREIGN_KEY_VIOLATION),
            "got: {error}"
        );
    }

    /// Seed three nodes whose cosine distance from `query` is exactly known,
    /// so a ranking assertion measures the metric rather than insertion order.
    async fn ranked_fixture(projector: &Projector, ledger: &LedgerStore, tenant: &str, repo: &str) {
        projected_nodes(
            projector,
            ledger,
            tenant,
            repo,
            &["artifact:near.rs", "artifact:mid.rs", "artifact:far.rs"],
        )
        .await;
        for (node_id, embedding) in [
            ("artifact:near.rs", sparse_unit(&[(0, 1.0)])),
            ("artifact:mid.rs", sparse_unit(&[(0, 1.0), (1, 1.0)])),
            ("artifact:far.rs", sparse_unit(&[(1, 1.0)])),
        ] {
            projector
                .upsert_embedding(tenant, repo, node_id, "nomic-embed-text", &embedding)
                .await
                .expect("embedding is accepted");
        }
    }

    /// The read half of ADR-0140: a query embedding ranks projected nodes by
    /// pgvector's cosine distance, computed inside PostgreSQL.
    #[tokio::test]
    async fn candidates_are_ranked_by_cosine_distance() {
        let url = require_test_database!();
        let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let ledger = LedgerStore::connect(&pool).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-a".to_string();
        ranked_fixture(&projector, &ledger, &tenant, &repo).await;

        let candidates = projector
            .similar_nodes(
                &tenant,
                &repo,
                "nomic-embed-text",
                &sparse_unit(&[(0, 1.0)]),
                10,
            )
            .await
            .expect("query succeeds");

        let ranked: Vec<String> = candidates.iter().map(|c| c.node_id.clone()).collect();
        assert_eq!(
            ranked,
            vec![
                "artifact:near.rs".to_string(),
                "artifact:mid.rs".to_string(),
                "artifact:far.rs".to_string(),
            ],
            "got: {candidates:?}"
        );
        // The distances themselves, not just their order -- an ordering-only
        // assertion would still pass if the query ranked by something else
        // that happened to agree.
        assert!(
            candidates[0].cosine_distance.abs() < 1e-5,
            "identical vectors should measure ~0.0, got: {candidates:?}"
        );
        assert!(
            (candidates[1].cosine_distance - 0.292_893).abs() < 1e-4,
            "45 degrees apart should measure ~0.2929, got: {candidates:?}"
        );
        assert!(
            (candidates[2].cosine_distance - 1.0).abs() < 1e-5,
            "orthogonal vectors should measure ~1.0, got: {candidates:?}"
        );
        // The join carried the projection's own columns through.
        assert_eq!(candidates[0].label, "artifact:near.rs");
        assert_eq!(candidates[0].node_type, "artifact");
    }

    /// ADR-0140 decision 1: a model a node was never embedded under degrades
    /// to no candidates rather than erroring, which is what lets a caller
    /// fall back to the recency/decay path (ADR-0080) instead of failing.
    #[tokio::test]
    async fn a_model_nothing_was_embedded_under_yields_no_candidates() {
        let url = require_test_database!();
        let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let ledger = LedgerStore::connect(&pool).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-a".to_string();
        ranked_fixture(&projector, &ledger, &tenant, &repo).await;

        let candidates = projector
            .similar_nodes(
                &tenant,
                &repo,
                "a-model-never-used-here",
                &sparse_unit(&[(0, 1.0)]),
                10,
            )
            .await
            .expect("an unembedded model degrades rather than erroring");

        assert!(candidates.is_empty(), "got: {candidates:?}");
    }

    /// Stage one is a *bounded* candidate set (ADR-0140 decision 3): the
    /// limit truncates the ranking, keeping the nearest.
    #[tokio::test]
    async fn the_candidate_set_is_bounded_by_the_limit() {
        let url = require_test_database!();
        let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let ledger = LedgerStore::connect(&pool).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-a".to_string();
        ranked_fixture(&projector, &ledger, &tenant, &repo).await;

        let candidates = projector
            .similar_nodes(
                &tenant,
                &repo,
                "nomic-embed-text",
                &sparse_unit(&[(0, 1.0)]),
                2,
            )
            .await
            .expect("query succeeds");

        let ranked: Vec<String> = candidates.iter().map(|c| c.node_id.clone()).collect();
        assert_eq!(
            ranked,
            vec![
                "artifact:near.rs".to_string(),
                "artifact:mid.rs".to_string()
            ],
            "got: {candidates:?}"
        );
    }

    /// Recall is scoped to one repository's projection -- a near-identical
    /// node in a sibling repository is not a candidate.
    #[tokio::test]
    async fn candidates_are_scoped_to_one_repository() {
        let url = require_test_database!();
        let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let ledger = LedgerStore::connect(&pool).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());

        projected_nodes(
            &projector,
            &ledger,
            &tenant,
            "repo-a",
            &["artifact:mine.rs"],
        )
        .await;
        projected_nodes(
            &projector,
            &ledger,
            &tenant,
            "repo-b",
            &["artifact:theirs.rs"],
        )
        .await;
        for (repo, node_id) in [
            ("repo-a", "artifact:mine.rs"),
            ("repo-b", "artifact:theirs.rs"),
        ] {
            projector
                .upsert_embedding(
                    &tenant,
                    repo,
                    node_id,
                    "nomic-embed-text",
                    &sparse_unit(&[(0, 1.0)]),
                )
                .await
                .expect("embedding is accepted");
        }

        let candidates = projector
            .similar_nodes(
                &tenant,
                "repo-a",
                "nomic-embed-text",
                &sparse_unit(&[(0, 1.0)]),
                10,
            )
            .await
            .expect("query succeeds");

        let ranked: Vec<String> = candidates.iter().map(|c| c.node_id.clone()).collect();
        assert_eq!(
            ranked,
            vec!["artifact:mine.rs".to_string()],
            "got: {candidates:?}"
        );
    }
}

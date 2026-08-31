use pgvector::Vector;

use super::*;

/// A projected node with no embedding for `model` yet (ADR-0140 decision 2).
#[derive(Debug, Clone, PartialEq)]
pub struct UnembeddedNode {
    pub node_id: String,
    pub label: String,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{DedupKey, LedgerStore};
    use crate::projection::tests::{require_test_database, structural_fact_envelope};
    use crate::test_support::uuid_ish;

    async fn projected_node(
        projector: &Projector,
        ledger: &mut LedgerStore,
        tenant: &str,
        repo: &str,
        node_id: &str,
    ) {
        let fact = StructuralFact {
            node_id: node_id.to_string(),
            node_type: "artifact".to_string(),
            label: node_id.to_string(),
            edges: vec![],
        };
        ledger
            .append(&structural_fact_envelope(
                DedupKey {
                    tenant_id: tenant.to_string(),
                    repository_id: repo.to_string(),
                    producer_id: "producer-a".to_string(),
                    producer_sequence: 1,
                },
                b"digest-1",
                &fact,
            ))
            .await
            .expect("append structural fact");
        projector
            .rebuild(tenant, repo)
            .await
            .expect("rebuild projects the node");
    }

    #[tokio::test]
    async fn a_projected_node_with_no_embedding_is_reported_missing() {
        let url = require_test_database!();
        let pool = crate::db_pool::build_pool(&url, crate::db_pool::TEST_POOL_MAX_SIZE)
            .expect("the test database url should build a pool");
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-a".to_string();

        projected_node(
            &projector,
            &mut ledger,
            &tenant,
            &repo,
            "artifact:src/lib.rs",
        )
        .await;

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
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-a".to_string();

        projected_node(
            &projector,
            &mut ledger,
            &tenant,
            &repo,
            "artifact:src/lib.rs",
        )
        .await;
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
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-a".to_string();

        projected_node(
            &projector,
            &mut ledger,
            &tenant,
            &repo,
            "artifact:src/lib.rs",
        )
        .await;
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
        let mut ledger = LedgerStore::connect(&url).await.expect("connect ledger");
        let projector = Projector::connect(&pool).await.expect("connect projector");
        let tenant = format!("t-{}", uuid_ish());
        let repo = "repo-a".to_string();

        projected_node(
            &projector,
            &mut ledger,
            &tenant,
            &repo,
            "artifact:src/lib.rs",
        )
        .await;
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
}

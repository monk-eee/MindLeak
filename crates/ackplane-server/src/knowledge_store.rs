//! Ackplane's PostgreSQL-backed knowledge domain, first slice (ADR-0106
//! decision 3): record, recall, and retire a learned-knowledge statement.
//!
//! Embeddings are ranked through pgvector's own `<=>` operator inside
//! Postgres, not pulled into application memory for a cosine loop over a
//! BLOB -- the exact SQLite scaling limit this domain exists to not repeat
//! (lodestar-core's own `knowledge_embeddings` table does the latter, by
//! necessity: SQLite has no native vector type). Effective weight is
//! computed by the same query, from `mindleak-core::decay::effective_weight`'s
//! own formula (`W_eff = W_base * 2^(-Δt_hours / half_life)`) expressed as
//! SQL -- derived at read time, never stored, matching this repository's
//! standing decay invariant.

use std::time::SystemTime;

use tokio_postgres::{Client, NoTls};

const MIGRATION: &str = include_str!("../migrations/0007_knowledge.sql");
const NONCE_MIGRATION: &str =
    include_str!("../migrations/0008_knowledge_authentication_nonces.sql");

#[derive(Debug, thiserror::Error)]
pub enum KnowledgeStoreError {
    #[error("knowledge database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("half_life_hours must be greater than zero")]
    InvalidHalfLife,
    #[error("content must not be empty")]
    EmptyContent,
}

/// One learned-knowledge statement as `record`/`retire` return it.
#[derive(Debug, Clone, PartialEq)]
pub struct Knowledge {
    pub knowledge_id: String,
    pub tenant_id: String,
    pub repository_id: String,
    pub content: String,
    pub source_ref: Option<String>,
    pub half_life_hours: f64,
    pub confirmed_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordKnowledgeRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub content: String,
    pub source_ref: Option<String>,
    pub half_life_hours: f64,
    /// `(model, embedding)`, when the caller has an embedder configured.
    /// Knowledge recorded without one still recalls, by recency (ADR-0080's
    /// same graceful degradation).
    pub embedding: Option<(String, Vec<f32>)>,
}

/// One recalled statement plus the fact a reader would otherwise have to
/// derive itself: its effective weight right now.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveKnowledge {
    pub knowledge_id: String,
    pub content: String,
    pub source_ref: Option<String>,
    pub effective_weight: f64,
    pub confirmed_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecallResult {
    pub entries: Vec<ActiveKnowledge>,
    /// False when no query embedding was supplied, or it named a model
    /// nothing here was embedded under -- `entries` still returns,
    /// effective-weight ordered, rather than being withheld.
    pub ranked_by_similarity: bool,
}

/// The same decay expression as `mindleak-core::decay::effective_weight`,
/// computed by Postgres at read time. A half-life of zero or less means
/// "never decays" (the base weight, 1.0 in this first slice -- no
/// reinforcement/boost tracking yet). Elapsed time at or below zero -- a
/// `confirmed_at` at or after Postgres's own `now()`, which real clock and
/// scheduling skew can produce even for a value genuinely written slightly
/// in the past -- also clamps to the base weight, mirroring
/// `mindleak-core::decay::effective_weight`'s own `if dt_hours <= 0.0 {
/// return base; }` guard exactly. Without this branch, a negative elapsed
/// time makes `power(2.0, positive exponent)` compute above 1.0.
const EFFECTIVE_WEIGHT_SQL: &str = "CASE WHEN half_life_hours <= 0 THEN 1.0 \
     WHEN now() <= confirmed_at THEN 1.0 \
     ELSE power(2.0, -(extract(epoch from (now() - confirmed_at)) / 3600.0) / half_life_hours) \
     END";

pub struct KnowledgeStore {
    client: Client,
}

impl KnowledgeStore {
    pub async fn connect(database_url: &str) -> Result<Self, tokio_postgres::Error> {
        let (mut client, connection) = tokio_postgres::connect(database_url, NoTls).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                tracing::error!(%error, "ackplane knowledge store connection closed with an error");
            }
        });
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::KNOWLEDGE,
            MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::KNOWLEDGE_AUTHENTICATION_NONCES,
            NONCE_MIGRATION,
        )
        .await?;
        Ok(Self { client })
    }

    /// Resolve the signing key a knowledge request's authentication claims,
    /// judged as of now. Mirrors `ClaimStore::resolve_signing_key`: the
    /// decision itself lives in `signing_keys` and is pure, this only owns
    /// the connection.
    pub async fn resolve_signing_key(
        &self,
        binding: &crate::signing_keys::EnvelopeBinding<'_>,
    ) -> Result<crate::signing_keys::KeyResolution, crate::signing_keys::SigningKeyError> {
        crate::signing_keys::resolve(&self.client, binding).await
    }

    /// Consume a (signing_key_id, nonce) pair exactly once (anti-replay for
    /// `KnowledgeGrpcService` authentication, ADR-0108 decision 3). Returns
    /// true the first time a pair is seen, false on every later attempt with
    /// the identical pair -- the insert's own uniqueness is the enforcement,
    /// so this needs no read-then-write race.
    pub async fn consume_knowledge_nonce(
        &self,
        signing_key_id: &str,
        nonce: &[u8],
        now: SystemTime,
    ) -> Result<bool, KnowledgeStoreError> {
        let inserted = self
            .client
            .execute(
                "INSERT INTO knowledge_authentication_nonces (signing_key_id, nonce, consumed_at) \
                 VALUES ($1, $2, $3) ON CONFLICT (signing_key_id, nonce) DO NOTHING",
                &[&signing_key_id, &nonce, &now],
            )
            .await?;
        Ok(inserted == 1)
    }

    pub async fn record(
        &self,
        request: RecordKnowledgeRequest,
    ) -> Result<Knowledge, KnowledgeStoreError> {
        if request.content.trim().is_empty() {
            return Err(KnowledgeStoreError::EmptyContent);
        }
        if request.half_life_hours <= 0.0 {
            return Err(KnowledgeStoreError::InvalidHalfLife);
        }
        let knowledge_id = unique_knowledge_id();
        let confirmed_at = SystemTime::now();
        self.client
            .execute(
                "INSERT INTO knowledge \
                 (tenant_id, repository_id, knowledge_id, content, source_ref, half_life_hours, confirmed_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &knowledge_id,
                    &request.content,
                    &request.source_ref,
                    &request.half_life_hours,
                    &confirmed_at,
                ],
            )
            .await?;
        if let Some((model, embedding)) = &request.embedding {
            self.client
                .execute(
                    "INSERT INTO knowledge_embeddings \
                     (tenant_id, repository_id, knowledge_id, model, embedding) \
                     VALUES ($1, $2, $3, $4, $5)",
                    &[
                        &request.tenant_id,
                        &request.repository_id,
                        &knowledge_id,
                        model,
                        &pgvector::Vector::from(embedding.clone()),
                    ],
                )
                .await?;
        }
        Ok(Knowledge {
            knowledge_id,
            tenant_id: request.tenant_id,
            repository_id: request.repository_id,
            content: request.content,
            source_ref: request.source_ref,
            half_life_hours: request.half_life_hours,
            confirmed_at,
        })
    }

    /// `embedding` ranks by pgvector similarity (`e.embedding <=> $query`)
    /// under `model`; without one, entries recall by effective weight
    /// (recency, decay-adjusted) instead.
    pub async fn recall(
        &self,
        tenant_id: &str,
        repository_id: &str,
        embedding: Option<(&str, Vec<f32>)>,
        limit: i64,
    ) -> Result<RecallResult, KnowledgeStoreError> {
        let ranked_by_similarity = embedding.is_some();
        let rows = match embedding {
            Some((model, query)) => {
                let query = pgvector::Vector::from(query);
                self.client
                    .query(
                        &format!(
                            "SELECT k.knowledge_id, k.content, k.source_ref, k.confirmed_at, \
                                    {EFFECTIVE_WEIGHT_SQL} AS effective_weight \
                             FROM knowledge k \
                             JOIN knowledge_embeddings e \
                               ON e.tenant_id = k.tenant_id AND e.repository_id = k.repository_id \
                                  AND e.knowledge_id = k.knowledge_id AND e.model = $3 \
                             WHERE k.tenant_id = $1 AND k.repository_id = $2 AND k.retired_at IS NULL \
                             ORDER BY e.embedding <=> $4 \
                             LIMIT $5"
                        ),
                        &[&tenant_id, &repository_id, &model, &query, &limit],
                    )
                    .await?
            }
            None => {
                self.client
                    .query(
                        &format!(
                            "SELECT knowledge_id, content, source_ref, confirmed_at, \
                                    {EFFECTIVE_WEIGHT_SQL} AS effective_weight \
                             FROM knowledge \
                             WHERE tenant_id = $1 AND repository_id = $2 AND retired_at IS NULL \
                             ORDER BY effective_weight DESC \
                             LIMIT $3"
                        ),
                        &[&tenant_id, &repository_id, &limit],
                    )
                    .await?
            }
        };
        let entries = rows
            .into_iter()
            .map(|row| ActiveKnowledge {
                knowledge_id: row.get("knowledge_id"),
                content: row.get("content"),
                source_ref: row.get("source_ref"),
                effective_weight: row.get("effective_weight"),
                confirmed_at: row.get("confirmed_at"),
            })
            .collect();
        Ok(RecallResult {
            entries,
            ranked_by_similarity,
        })
    }

    pub async fn retire(
        &self,
        tenant_id: &str,
        repository_id: &str,
        knowledge_id: &str,
        reason: &str,
        retired_by: &str,
    ) -> Result<bool, KnowledgeStoreError> {
        let updated = self
            .client
            .execute(
                "UPDATE knowledge SET retired_at = now(), retired_reason = $4, retired_by = $5 \
                 WHERE tenant_id = $1 AND repository_id = $2 AND knowledge_id = $3 AND retired_at IS NULL",
                &[&tenant_id, &repository_id, &knowledge_id, &reason, &retired_by],
            )
            .await?;
        Ok(updated > 0)
    }
}

/// A random, prefixed id -- no meaning is derived from its bytes, unlike
/// `(tenant_id, repository_id)` which stay the real scoping key everywhere.
fn unique_knowledge_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("the OS random source should be available");
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("knowledge-{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_scope(label: &str) -> (String, String) {
        let mut bytes = [0u8; 8];
        getrandom::getrandom(&mut bytes).expect("the OS random source should be available");
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        (
            format!("tenant-{label}-{hex}"),
            format!("repo-{label}-{hex}"),
        )
    }

    async fn store() -> Option<KnowledgeStore> {
        let database_url = std::env::var("ACKPLANE_TEST_DATABASE_URL").ok()?;
        Some(KnowledgeStore::connect(&database_url).await.unwrap())
    }

    #[tokio::test]
    async fn a_recorded_statement_recalls_by_recency_with_no_embedding() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("recency");
        let recorded = store
            .record(RecordKnowledgeRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                content: "the migration lock key must be unique per file".to_string(),
                source_ref: Some("pr:538".to_string()),
                half_life_hours: 720.0,
                embedding: None,
            })
            .await
            .unwrap();

        let recalled = store
            .recall(&tenant_id, &repository_id, None, 10)
            .await
            .unwrap();

        assert!(!recalled.ranked_by_similarity);
        assert_eq!(recalled.entries.len(), 1);
        assert_eq!(recalled.entries[0].knowledge_id, recorded.knowledge_id);
        assert_eq!(
            recalled.entries[0].content,
            "the migration lock key must be unique per file"
        );
        assert!(recalled.entries[0].effective_weight > 0.99);
        assert!(recalled.entries[0].effective_weight <= 1.0);
    }

    /// A `confirmed_at` at or after Postgres's own `now()` -- exactly what
    /// real clock/scheduling skew occasionally produces for a value written
    /// only moments earlier (this repository's own documented
    /// contention-flake class) -- must clamp to the base weight, not exceed
    /// it. Reproduced deterministically with a future `confirmed_at`
    /// inserted directly, rather than waiting for real timing jitter to
    /// (rarely) hit the same condition. Sabotage-verified: reverting
    /// `EFFECTIVE_WEIGHT_SQL`'s `now() <= confirmed_at` branch makes this
    /// fail (`power(2.0, positive exponent)` computes above `1.0`).
    #[tokio::test]
    async fn recall_clamps_effective_weight_when_confirmed_at_is_at_or_after_now() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let store = KnowledgeStore::connect(&database_url)
            .await
            .expect("connect knowledge store");
        let (tenant_id, repository_id) = unique_scope("future-confirmed");
        let knowledge_id = unique_knowledge_id();

        let (client, connection) = tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
            .await
            .expect("raw connection for the sabotage insert");
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client
            .execute(
                "INSERT INTO knowledge \
                 (tenant_id, repository_id, knowledge_id, content, source_ref, half_life_hours, confirmed_at) \
                 VALUES ($1, $2, $3, $4, NULL, $5, now() + interval '1 hour')",
                &[
                    &tenant_id,
                    &repository_id,
                    &knowledge_id,
                    &"a statement confirmed after the database's own clock",
                    &720.0_f64,
                ],
            )
            .await
            .expect("insert a future-confirmed statement");

        let recalled = store
            .recall(&tenant_id, &repository_id, None, 10)
            .await
            .expect("query recall");

        assert_eq!(recalled.entries.len(), 1);
        assert_eq!(recalled.entries[0].knowledge_id, knowledge_id);
        assert_eq!(
            recalled.entries[0].effective_weight, 1.0,
            "a future confirmed_at must clamp to the base weight, not exceed it"
        );
    }

    #[tokio::test]
    async fn effective_weight_derives_from_half_life_and_elapsed_time() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("decay");
        // ~0.36s half-life: after a real 2s sleep this must have decayed
        // hard, proving effective_weight is actually derived from
        // half_life_hours and elapsed time, not a constant the query
        // always returns.
        let fast = store
            .record(RecordKnowledgeRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                content: "fast-decaying".to_string(),
                source_ref: None,
                half_life_hours: 0.0001,
                embedding: None,
            })
            .await
            .unwrap();
        let slow = store
            .record(RecordKnowledgeRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                content: "slow-decaying".to_string(),
                source_ref: None,
                half_life_hours: 720.0,
                embedding: None,
            })
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let recalled = store
            .recall(&tenant_id, &repository_id, None, 10)
            .await
            .unwrap();
        let fast_weight = recalled
            .entries
            .iter()
            .find(|entry| entry.knowledge_id == fast.knowledge_id)
            .unwrap()
            .effective_weight;
        let slow_weight = recalled
            .entries
            .iter()
            .find(|entry| entry.knowledge_id == slow.knowledge_id)
            .unwrap()
            .effective_weight;

        assert!(
            fast_weight < 0.1,
            "a ~0.36s half-life should have decayed hard after 2s: got {fast_weight}"
        );
        assert!(
            slow_weight > 0.99,
            "a 30-day half-life should barely move after 2s: got {slow_weight}"
        );
    }

    #[tokio::test]
    async fn a_non_positive_half_life_is_refused() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("invalid-half-life");
        let error = store
            .record(RecordKnowledgeRequest {
                tenant_id,
                repository_id,
                content: "should not record".to_string(),
                source_ref: None,
                half_life_hours: 0.0,
                embedding: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(error, KnowledgeStoreError::InvalidHalfLife));
    }

    #[tokio::test]
    async fn retiring_removes_a_statement_from_recall_but_not_the_record() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("retire");
        let recorded = store
            .record(RecordKnowledgeRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                content: "superseded guidance".to_string(),
                source_ref: None,
                half_life_hours: 24.0,
                embedding: None,
            })
            .await
            .unwrap();

        let retired = store
            .retire(
                &tenant_id,
                &repository_id,
                &recorded.knowledge_id,
                "measured false",
                "Reviewer",
            )
            .await
            .unwrap();
        assert!(retired);

        let recalled = store
            .recall(&tenant_id, &repository_id, None, 10)
            .await
            .unwrap();
        assert_eq!(recalled.entries.len(), 0);

        // Retiring an already-retired (or nonexistent) id is reported, not
        // an error -- the caller can tell "nothing changed" from "it broke".
        let retired_again = store
            .retire(
                &tenant_id,
                &repository_id,
                &recorded.knowledge_id,
                "measured false",
                "Reviewer",
            )
            .await
            .unwrap();
        assert!(!retired_again);
    }

    #[tokio::test]
    async fn recall_ranks_by_pgvector_similarity_when_an_embedding_is_supplied() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("semantic");
        let model = "test-embedder";
        let closest = store
            .record(RecordKnowledgeRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                content: "closest match".to_string(),
                source_ref: None,
                half_life_hours: 720.0,
                embedding: Some((model.to_string(), vec![1.0; 768])),
            })
            .await
            .unwrap();
        store
            .record(RecordKnowledgeRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                content: "farthest match".to_string(),
                source_ref: None,
                half_life_hours: 720.0,
                embedding: Some((model.to_string(), {
                    let mut v = vec![1.0; 768];
                    v[0] = -1.0;
                    v
                })),
            })
            .await
            .unwrap();

        let recalled = store
            .recall(
                &tenant_id,
                &repository_id,
                Some((model, vec![1.0; 768])),
                10,
            )
            .await
            .unwrap();

        assert!(recalled.ranked_by_similarity);
        assert_eq!(recalled.entries.len(), 2);
        assert_eq!(recalled.entries[0].knowledge_id, closest.knowledge_id);
    }

    #[tokio::test]
    async fn recall_under_a_model_nothing_was_embedded_with_still_returns_by_recency() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("no-model-match");
        store
            .record(RecordKnowledgeRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                content: "embedded under a different model".to_string(),
                source_ref: None,
                half_life_hours: 720.0,
                embedding: Some(("other-model".to_string(), vec![0.5; 768])),
            })
            .await
            .unwrap();

        let recalled = store
            .recall(
                &tenant_id,
                &repository_id,
                Some(("nomic-embed-text", vec![0.5; 768])),
                10,
            )
            .await
            .unwrap();

        // The JOIN on model finds nothing, so the semantic path legitimately
        // returns zero rows -- this is honest degradation (ranked_by_similarity
        // stays true because a query WAS supplied), not a silent fallback that
        // would hide the mismatch.
        assert_eq!(recalled.entries.len(), 0);
    }
}

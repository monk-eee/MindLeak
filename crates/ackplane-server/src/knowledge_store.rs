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
const RECORDED_BY_MIGRATION: &str = include_str!("../migrations/0011_knowledge_recorded_by.sql");
const RECONFIRMATION_MIGRATION: &str =
    include_str!("../migrations/0012_knowledge_reconfirmations.sql");
const REACH_MIGRATION: &str = include_str!("../migrations/0013_knowledge_reach.sql");

mod reach;
mod reconfirmation;

pub use reconfirmation::KnowledgeReconfirmation;

use reach::validate_reach;

#[derive(Debug, thiserror::Error)]
pub enum KnowledgeStoreError {
    #[error("knowledge database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("half_life_hours must be greater than zero")]
    InvalidHalfLife,
    #[error("content must not be empty")]
    EmptyContent,
    #[error("reconfirmation evidence must not be empty")]
    EmptyReconfirmationEvidence,
    #[error("reach node must be a repository-relative artifact: or symbol: id: {0}")]
    InvalidReachNode(String),
    #[error("reach node ids must be unique: {0}")]
    DuplicateReachNode(String),
    #[error("reach goal must be a non-empty goal: identifier")]
    InvalidReachGoal,
}

/// One learned-knowledge statement as `record`/`retire` return it.
#[derive(Debug, Clone, PartialEq)]
pub struct Knowledge {
    pub knowledge_id: String,
    pub tenant_id: String,
    pub repository_id: String,
    pub content: String,
    pub source_ref: Option<String>,
    pub recorded_by: Option<String>,
    pub reach_node_ids: Vec<String>,
    pub reach_goal_id: Option<String>,
    pub half_life_hours: f64,
    pub confirmed_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecordKnowledgeRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub content: String,
    pub source_ref: Option<String>,
    /// The authenticated enrolled node when the caller has one. `None`
    /// retains the truthful lack of provenance for a legacy/imported row.
    pub recorded_by: Option<String>,
    pub reach_node_ids: Vec<String>,
    pub reach_goal_id: Option<String>,
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
    pub recorded_by: Option<String>,
    pub reach_node_ids: Vec<String>,
    pub reach_goal_id: Option<String>,
    pub last_reconfirmed_at: Option<SystemTime>,
    pub last_reconfirmed_by: Option<String>,
    pub last_reconfirmation_evidence_ref: Option<String>,
    pub effective_weight: f64,
    pub confirmed_at: SystemTime,
}

/// One knowledge statement's lifecycle, including an attributed retirement
/// when the statement is no longer active.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeHistoryEntry {
    pub knowledge_id: String,
    pub content: String,
    pub source_ref: Option<String>,
    pub recorded_by: Option<String>,
    pub reach_node_ids: Vec<String>,
    pub reach_goal_id: Option<String>,
    pub last_reconfirmed_at: Option<SystemTime>,
    pub last_reconfirmed_by: Option<String>,
    pub last_reconfirmation_evidence_ref: Option<String>,
    pub confirmed_at: SystemTime,
    pub retired_at: Option<SystemTime>,
    pub retired_reason: Option<String>,
    pub retired_by: Option<String>,
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

const LATEST_RECONFIRMATION_JOIN: &str = "LEFT JOIN LATERAL ( \
        SELECT reconfirmed_at AS last_reconfirmed_at, \
                     reconfirmed_by AS last_reconfirmed_by, \
                     evidence_ref AS last_reconfirmation_evidence_ref \
            FROM knowledge_reconfirmations \
         WHERE tenant_id = k.tenant_id \
             AND repository_id = k.repository_id \
             AND knowledge_id = k.knowledge_id \
         ORDER BY reconfirmed_at DESC, reconfirmation_id DESC \
         LIMIT 1 \
 ) latest_reconfirmation ON TRUE";

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
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::KNOWLEDGE_RECORDED_BY,
            RECORDED_BY_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::KNOWLEDGE_RECONFIRMATIONS,
            RECONFIRMATION_MIGRATION,
        )
        .await?;
        crate::migration_lock::migrate_locked(
            &mut client,
            crate::migration_lock::key::KNOWLEDGE_REACH,
            REACH_MIGRATION,
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
        validate_reach(&request.reach_node_ids, request.reach_goal_id.as_deref())?;
        let knowledge_id = unique_knowledge_id();
        let confirmed_at = SystemTime::now();
        self.client
            .execute(
                "INSERT INTO knowledge \
                 (tenant_id, repository_id, knowledge_id, content, source_ref, recorded_by, reach_node_ids, reach_goal_id, half_life_hours, confirmed_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
                &[
                    &request.tenant_id,
                    &request.repository_id,
                    &knowledge_id,
                    &request.content,
                    &request.source_ref,
                    &request.recorded_by,
                    &request.reach_node_ids,
                    &request.reach_goal_id,
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
            recorded_by: request.recorded_by,
            reach_node_ids: request.reach_node_ids,
            reach_goal_id: request.reach_goal_id,
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
                            "SELECT k.knowledge_id, k.content, k.source_ref, k.recorded_by, k.reach_node_ids, k.reach_goal_id, k.confirmed_at, \
                                latest_reconfirmation.last_reconfirmed_at, \
                                latest_reconfirmation.last_reconfirmed_by, \
                                latest_reconfirmation.last_reconfirmation_evidence_ref, \
                                    {EFFECTIVE_WEIGHT_SQL} AS effective_weight \
                             FROM knowledge k \
                             JOIN knowledge_embeddings e \
                               ON e.tenant_id = k.tenant_id AND e.repository_id = k.repository_id \
                                  AND e.knowledge_id = k.knowledge_id AND e.model = $3 \
                             {LATEST_RECONFIRMATION_JOIN} \
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
                            "SELECT k.knowledge_id, k.content, k.source_ref, k.recorded_by, k.reach_node_ids, k.reach_goal_id, k.confirmed_at, \
                                latest_reconfirmation.last_reconfirmed_at, \
                                latest_reconfirmation.last_reconfirmed_by, \
                                latest_reconfirmation.last_reconfirmation_evidence_ref, \
                                    {EFFECTIVE_WEIGHT_SQL} AS effective_weight \
                             FROM knowledge k \
                             {LATEST_RECONFIRMATION_JOIN} \
                             WHERE k.tenant_id = $1 AND k.repository_id = $2 AND k.retired_at IS NULL \
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
                recorded_by: row.get("recorded_by"),
                reach_node_ids: row.get("reach_node_ids"),
                reach_goal_id: row.get("reach_goal_id"),
                last_reconfirmed_at: row.get("last_reconfirmed_at"),
                last_reconfirmed_by: row.get("last_reconfirmed_by"),
                last_reconfirmation_evidence_ref: row.get("last_reconfirmation_evidence_ref"),
                effective_weight: row.get("effective_weight"),
                confirmed_at: row.get("confirmed_at"),
            })
            .collect();
        Ok(RecallResult {
            entries,
            ranked_by_similarity,
        })
    }

    /// Returns active and retired statements for one repository, preserving
    /// the retirement provenance needed to explain why guidance disappeared.
    pub async fn history(
        &self,
        tenant_id: &str,
        repository_id: &str,
        limit: i64,
    ) -> Result<Vec<KnowledgeHistoryEntry>, KnowledgeStoreError> {
        let rows = self
            .client
            .query(
                &format!(
                    "SELECT k.knowledge_id, k.content, k.source_ref, k.recorded_by, k.reach_node_ids, k.reach_goal_id, k.confirmed_at, \
                            k.retired_at, k.retired_reason, k.retired_by, \
                            latest_reconfirmation.last_reconfirmed_at, \
                            latest_reconfirmation.last_reconfirmed_by, \
                            latest_reconfirmation.last_reconfirmation_evidence_ref \
                     FROM knowledge k \
                     {LATEST_RECONFIRMATION_JOIN} \
                     WHERE k.tenant_id = $1 AND k.repository_id = $2 \
                     ORDER BY COALESCE(k.retired_at, k.confirmed_at) DESC, k.knowledge_id ASC \
                     LIMIT $3"
                ),
                &[&tenant_id, &repository_id, &limit],
            )
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| KnowledgeHistoryEntry {
                knowledge_id: row.get("knowledge_id"),
                content: row.get("content"),
                source_ref: row.get("source_ref"),
                recorded_by: row.get("recorded_by"),
                reach_node_ids: row.get("reach_node_ids"),
                reach_goal_id: row.get("reach_goal_id"),
                last_reconfirmed_at: row.get("last_reconfirmed_at"),
                last_reconfirmed_by: row.get("last_reconfirmed_by"),
                last_reconfirmation_evidence_ref: row.get("last_reconfirmation_evidence_ref"),
                confirmed_at: row.get("confirmed_at"),
                retired_at: row.get("retired_at"),
                retired_reason: row.get("retired_reason"),
                retired_by: row.get("retired_by"),
            })
            .collect())
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
        let expected_reach_node_ids = vec![
            "artifact:crates/ackplane-server/src/knowledge_store.rs".to_string(),
            "symbol:crates/ackplane-server/src/knowledge_store.rs:KnowledgeStore".to_string(),
        ];
        let expected_reach_goal_id = "goal:ackplane-federation-service";
        let recorded = store
            .record(RecordKnowledgeRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                content: "the migration lock key must be unique per file".to_string(),
                source_ref: Some("pr:538".to_string()),
                recorded_by: Some("node:recency".to_string()),
                reach_node_ids: expected_reach_node_ids.clone(),
                reach_goal_id: Some(expected_reach_goal_id.to_string()),
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
        assert_eq!(recorded.recorded_by.as_deref(), Some("node:recency"));
        assert_eq!(
            recalled.entries[0].recorded_by.as_deref(),
            Some("node:recency")
        );
        assert_eq!(recorded.reach_node_ids, expected_reach_node_ids);
        assert_eq!(
            recorded.reach_goal_id.as_deref(),
            Some(expected_reach_goal_id)
        );
        assert_eq!(recalled.entries[0].reach_node_ids, expected_reach_node_ids);
        assert_eq!(
            recalled.entries[0].reach_goal_id.as_deref(),
            Some(expected_reach_goal_id)
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
                recorded_by: None,
                reach_node_ids: Vec::new(),
                reach_goal_id: None,
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
                recorded_by: None,
                reach_node_ids: Vec::new(),
                reach_goal_id: None,
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
                recorded_by: None,
                reach_node_ids: Vec::new(),
                reach_goal_id: None,
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
                recorded_by: None,
                reach_node_ids: Vec::new(),
                reach_goal_id: None,
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
    async fn history_keeps_retirement_provenance_within_its_tenant_and_repository() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("history");
        let active = store
            .record(RecordKnowledgeRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                content: "still active".to_string(),
                source_ref: Some("test:active".to_string()),
                recorded_by: Some("node:active".to_string()),
                reach_node_ids: Vec::new(),
                reach_goal_id: None,
                half_life_hours: 24.0,
                embedding: None,
            })
            .await
            .unwrap();
        let retired = store
            .record(RecordKnowledgeRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                content: "superseded".to_string(),
                source_ref: Some("test:retired".to_string()),
                recorded_by: Some("node:retired".to_string()),
                reach_node_ids: Vec::new(),
                reach_goal_id: None,
                half_life_hours: 24.0,
                embedding: None,
            })
            .await
            .unwrap();
        let (other_tenant_id, other_repository_id) = unique_scope("history-other");
        store
            .record(RecordKnowledgeRequest {
                tenant_id: other_tenant_id,
                repository_id: other_repository_id,
                content: "must not cross tenant boundaries".to_string(),
                source_ref: None,
                recorded_by: Some("node:other".to_string()),
                reach_node_ids: Vec::new(),
                reach_goal_id: None,
                half_life_hours: 24.0,
                embedding: None,
            })
            .await
            .unwrap();
        store
            .retire(
                &tenant_id,
                &repository_id,
                &retired.knowledge_id,
                "superseded by newer evidence",
                "node:reviewer",
            )
            .await
            .unwrap();

        let history = store.history(&tenant_id, &repository_id, 10).await.unwrap();
        assert_eq!(history.len(), 2);

        let active_entry = history
            .iter()
            .find(|entry| entry.knowledge_id == active.knowledge_id)
            .unwrap();
        assert_eq!(active_entry.content, "still active");
        assert_eq!(active_entry.retired_at, None);
        assert_eq!(active_entry.retired_reason, None);
        assert_eq!(active_entry.retired_by, None);
        assert_eq!(active_entry.recorded_by.as_deref(), Some("node:active"));

        let retired_entry = history
            .iter()
            .find(|entry| entry.knowledge_id == retired.knowledge_id)
            .unwrap();
        assert_eq!(retired_entry.content, "superseded");
        assert!(retired_entry.retired_at.is_some());
        assert_eq!(
            retired_entry.retired_reason.as_deref(),
            Some("superseded by newer evidence")
        );
        assert_eq!(retired_entry.retired_by.as_deref(), Some("node:reviewer"));
        assert_eq!(retired_entry.recorded_by.as_deref(), Some("node:retired"));
    }

    #[tokio::test]
    async fn reconfirmation_resets_an_active_statements_clock_with_audited_evidence() {
        let Some(store) = store().await else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let (tenant_id, repository_id) = unique_scope("reconfirm");
        let recorded = store
            .record(RecordKnowledgeRequest {
                tenant_id: tenant_id.clone(),
                repository_id: repository_id.clone(),
                content: "reconfirmation resets decay".to_string(),
                source_ref: Some("evidence:initial".to_string()),
                recorded_by: Some("node:recorder".to_string()),
                reach_node_ids: Vec::new(),
                reach_goal_id: None,
                half_life_hours: 24.0,
                embedding: None,
            })
            .await
            .unwrap();
        let reconfirmed_at = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_234_567);

        let reconfirmation = store
            .reconfirm(
                &tenant_id,
                &repository_id,
                &recorded.knowledge_id,
                "evidence:corroborated",
                "node:reviewer",
                reconfirmed_at,
            )
            .await
            .unwrap()
            .expect("an active statement should be reconfirmed");
        assert_eq!(reconfirmation.evidence_ref, "evidence:corroborated");
        assert_eq!(reconfirmation.reconfirmed_by, "node:reviewer");
        assert_eq!(reconfirmation.reconfirmed_at, reconfirmed_at);

        let history = store.history(&tenant_id, &repository_id, 10).await.unwrap();
        assert_eq!(history.len(), 1);
        let entry = &history[0];
        assert_eq!(entry.knowledge_id, recorded.knowledge_id);
        assert_eq!(entry.confirmed_at, reconfirmed_at);
        assert_eq!(entry.last_reconfirmed_at, Some(reconfirmed_at));
        assert_eq!(
            entry.last_reconfirmation_evidence_ref.as_deref(),
            Some("evidence:corroborated")
        );
        assert_eq!(entry.last_reconfirmed_by.as_deref(), Some("node:reviewer"));
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
                recorded_by: None,
                reach_node_ids: Vec::new(),
                reach_goal_id: None,
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
                recorded_by: None,
                reach_node_ids: Vec::new(),
                reach_goal_id: None,
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
                recorded_by: None,
                reach_node_ids: Vec::new(),
                reach_goal_id: None,
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

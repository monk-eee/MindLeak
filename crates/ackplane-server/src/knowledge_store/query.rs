use super::{
    ActiveKnowledge, KnowledgeHistoryEntry, KnowledgeStore, KnowledgeStoreError, RecallResult,
};

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

impl KnowledgeStore {
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
}

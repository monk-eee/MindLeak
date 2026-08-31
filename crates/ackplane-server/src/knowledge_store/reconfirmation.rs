use std::time::SystemTime;

use super::{KnowledgeLifecycleState, KnowledgeStore, KnowledgeStoreError};

/// One durable corroboration that refreshed a knowledge statement's decay
/// clock. The latest entry is exposed on recall/history; this record keeps the
/// full audit chain instead of overwriting the prior corroboration.
#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeReconfirmation {
    pub reconfirmation_id: String,
    pub evidence_ref: String,
    pub reconfirmed_by: String,
    pub reconfirmed_at: SystemTime,
}

impl KnowledgeStore {
    /// Reconfirms a still-authoritative statement (`Candidate` or `Active`,
    /// per ADR-0113 decision 1's closed lifecycle vocabulary) with fresh
    /// corroborating evidence. The `WITH` statement updates its clock and
    /// inserts the audit event as one atomic operation, so a retired,
    /// superseded, or missing statement cannot acquire a reconfirmation
    /// record through an interleaving write. The state predicate deliberately
    /// enumerates `Candidate` and `Active`: `Superseded` leaves `retired_at`
    /// NULL, but has been explicitly replaced and cannot pass the authority
    /// guard.
    pub async fn reconfirm(
        &self,
        tenant_id: &str,
        repository_id: &str,
        knowledge_id: &str,
        evidence_ref: &str,
        reconfirmed_by: &str,
        now: SystemTime,
    ) -> Result<Option<KnowledgeReconfirmation>, KnowledgeStoreError> {
        let evidence_ref = evidence_ref.trim();
        if evidence_ref.is_empty() {
            return Err(KnowledgeStoreError::EmptyReconfirmationEvidence);
        }
        let reconfirmation_id = unique_reconfirmation_id();
        let row = self
            .connection()
            .await?
            .query_opt(
                "WITH refreshed AS ( \
                    UPDATE knowledge \
                       SET confirmed_at = $6 \
                     WHERE tenant_id = $1 AND repository_id = $2 AND knowledge_id = $3 \
                       AND retired_at IS NULL AND lifecycle_state IN ($8, $9) \
                 RETURNING knowledge_id \
                 ) \
                 INSERT INTO knowledge_reconfirmations \
                     (tenant_id, repository_id, knowledge_id, reconfirmation_id, evidence_ref, reconfirmed_by, reconfirmed_at) \
                 SELECT $1, $2, knowledge_id, $4, $5, $7, $6 FROM refreshed \
                 RETURNING reconfirmation_id, evidence_ref, reconfirmed_by, reconfirmed_at",
                &[
                    &tenant_id,
                    &repository_id,
                    &knowledge_id,
                    &reconfirmation_id,
                    &evidence_ref,
                    &now,
                    &reconfirmed_by,
                    &(KnowledgeLifecycleState::Candidate as i16),
                    &(KnowledgeLifecycleState::Active as i16),
                ],
            )
            .await?;
        Ok(row.map(|row| KnowledgeReconfirmation {
            reconfirmation_id: row.get("reconfirmation_id"),
            evidence_ref: row.get("evidence_ref"),
            reconfirmed_by: row.get("reconfirmed_by"),
            reconfirmed_at: row.get("reconfirmed_at"),
        }))
    }
}

fn unique_reconfirmation_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("the OS random source should be available");
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("knowledge-reconfirmation-{hex}")
}

use std::time::SystemTime;

use super::reach::validate_reach;
use super::{
    unique_knowledge_id, Knowledge, KnowledgeLifecycleState, KnowledgeStore, KnowledgeStoreError,
    RecordKnowledgeRequest,
};

impl KnowledgeStore {
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
                 (tenant_id, repository_id, knowledge_id, content, source_ref, recorded_by, reach_node_ids, reach_goal_id, half_life_hours, confirmed_at, lifecycle_state) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
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
                    &(KnowledgeLifecycleState::Candidate as i16),
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
            lifecycle_state: KnowledgeLifecycleState::Candidate,
            superseded_by: None,
        })
    }
}

//! gRPC transport for Ackplane's knowledge domain (ADR-0106 decision 3).
//!
//! Unauthenticated in this first slice, unlike `ClaimDelegationService`:
//! `ClaimOperation`'s signing scheme is scoped to claim identity (task_id,
//! owner_id) that has no equivalent here, so reusing it would be a domain
//! mismatch, not reuse. Binding knowledge writes to the enrolled node's
//! signing key is real, separate follow-on work -- tracked in
//! gaps.d/ackplane-knowledge-service-rpcs-are-unauthenticated.md, not
//! silently deferred.

use std::sync::Arc;

use ackplane_protocol::v1;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use crate::knowledge_store::{
    ActiveKnowledge, KnowledgeStore, KnowledgeStoreError, RecordKnowledgeRequest,
};

pub struct KnowledgeGrpcService {
    store: Arc<Mutex<KnowledgeStore>>,
}

impl KnowledgeGrpcService {
    pub fn new(store: KnowledgeStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }
}

fn store_error(error: KnowledgeStoreError) -> Status {
    match error {
        KnowledgeStoreError::InvalidHalfLife => {
            Status::invalid_argument("half_life_hours must be greater than zero")
        }
        KnowledgeStoreError::EmptyContent => Status::invalid_argument("content must not be empty"),
        KnowledgeStoreError::Database(error) => Status::internal(error.to_string()),
    }
}

fn to_proto_entry(entry: ActiveKnowledge) -> Result<v1::ActiveKnowledgeEntry, String> {
    Ok(v1::ActiveKnowledgeEntry {
        knowledge_id: entry.knowledge_id,
        content: entry.content,
        source_ref: entry.source_ref.unwrap_or_default(),
        effective_weight: entry.effective_weight,
        confirmed_at: rfc3339(entry.confirmed_at)?,
    })
}

fn rfc3339(timestamp: std::time::SystemTime) -> Result<String, String> {
    time::OffsetDateTime::from(timestamp)
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| format!("could not format a knowledge timestamp: {error}"))
}

#[tonic::async_trait]
impl v1::knowledge_service_server::KnowledgeService for KnowledgeGrpcService {
    async fn record_knowledge(
        &self,
        request: Request<v1::RecordKnowledgeRequest>,
    ) -> Result<Response<v1::KnowledgeRecord>, Status> {
        let request = request.into_inner();
        let embedding = if request.embedding.is_empty() {
            None
        } else {
            Some((request.embedding_model, request.embedding))
        };
        let recorded = self
            .store
            .lock()
            .await
            .record(RecordKnowledgeRequest {
                tenant_id: request.tenant_id,
                repository_id: request.repository_id,
                content: request.content,
                source_ref: (!request.source_ref.is_empty()).then_some(request.source_ref),
                half_life_hours: request.half_life_hours,
                embedding,
            })
            .await
            .map_err(store_error)?;
        Ok(Response::new(v1::KnowledgeRecord {
            knowledge_id: recorded.knowledge_id,
            tenant_id: recorded.tenant_id,
            repository_id: recorded.repository_id,
            content: recorded.content,
            source_ref: recorded.source_ref.unwrap_or_default(),
            half_life_hours: recorded.half_life_hours,
            confirmed_at: rfc3339(recorded.confirmed_at).map_err(Status::internal)?,
        }))
    }

    async fn recall_knowledge(
        &self,
        request: Request<v1::RecallKnowledgeRequest>,
    ) -> Result<Response<v1::RecallKnowledgeResult>, Status> {
        let request = request.into_inner();
        let embedding = if request.query_embedding.is_empty() {
            None
        } else {
            Some((request.embedding_model.as_str(), request.query_embedding))
        };
        let limit = if request.limit == 0 {
            20
        } else {
            request.limit as i64
        };
        let recalled = self
            .store
            .lock()
            .await
            .recall(&request.tenant_id, &request.repository_id, embedding, limit)
            .await
            .map_err(store_error)?;
        Ok(Response::new(v1::RecallKnowledgeResult {
            entries: recalled
                .entries
                .into_iter()
                .map(to_proto_entry)
                .collect::<Result<Vec<_>, String>>()
                .map_err(Status::internal)?,
            ranked_by_similarity: recalled.ranked_by_similarity,
        }))
    }

    async fn retire_knowledge(
        &self,
        request: Request<v1::RetireKnowledgeRequest>,
    ) -> Result<Response<v1::RetireKnowledgeResult>, Status> {
        let request = request.into_inner();
        let retired = self
            .store
            .lock()
            .await
            .retire(
                &request.tenant_id,
                &request.repository_id,
                &request.knowledge_id,
                &request.reason,
                &request.retired_by,
            )
            .await
            .map_err(store_error)?;
        Ok(Response::new(v1::RetireKnowledgeResult { retired }))
    }
}

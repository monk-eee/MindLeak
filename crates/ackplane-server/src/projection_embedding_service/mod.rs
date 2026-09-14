use std::time::SystemTime;

use ackplane_protocol::{
    projection_embedding_auth::{ProjectionEmbeddingOperation, DEFAULT_EMBEDDING_LIST_LIMIT},
    v1::{self, projection_embedding_service_server::ProjectionEmbeddingServiceServer},
};
use prost::Message;
use tonic::{Request, Response, Status};

use crate::{
    projection::{ProjectionError, Projector, UnembeddedNode},
    projection_embedding_signature,
    signing_keys::EnvelopeBinding,
};

const MAX_SOURCE_PAGE_BYTES: usize = 192 * 1024;

mod recall;

pub struct ProjectionEmbeddingService {
    projector: Projector,
}

impl ProjectionEmbeddingService {
    pub fn new(projector: Projector) -> Self {
        Self { projector }
    }

    pub fn into_server(self) -> ProjectionEmbeddingServiceServer<Self> {
        ProjectionEmbeddingServiceServer::new(self)
            .max_decoding_message_size(64 * 1024)
            .max_encoding_message_size(1024 * 1024)
    }

    async fn authenticate(
        &self,
        tenant_id: &str,
        repository_id: &str,
        operation: &ProjectionEmbeddingOperation<'_>,
        authentication: Option<&v1::ProjectionEmbeddingAuthentication>,
    ) -> Result<(), Status> {
        let authentication = authentication.ok_or_else(|| {
            Status::unauthenticated("projection embedding authentication is required")
        })?;
        if tenant_id.trim().is_empty()
            || tenant_id.len() > 2048
            || repository_id.trim().is_empty()
            || repository_id.len() > 2048
            || authentication.signing_key_id.len() > 2048
            || authentication.node_id.len() > 2048
            || authentication.signed_at.len() > 64
        {
            return Err(Status::invalid_argument(
                "projection embedding identity exceeds its bounds",
            ));
        }
        let now = SystemTime::now();
        let resolution = self
            .projector
            .resolve_signing_key(&EnvelopeBinding {
                signing_key_id: &authentication.signing_key_id,
                tenant_id,
                repository_id,
                producer_id: &authentication.node_id,
                accepted_at: now,
            })
            .await
            .map_err(store_error)?;
        projection_embedding_signature::verify(
            tenant_id,
            repository_id,
            operation,
            authentication,
            &resolution,
            now,
        )?;
        if !self
            .projector
            .consume_embedding_nonce(&authentication.signing_key_id, &authentication.nonce, now)
            .await
            .map_err(store_error)?
        {
            return Err(Status::unauthenticated(
                "projection embedding authentication was already used",
            ));
        }
        Ok(())
    }
}

fn store_error(error: ProjectionError) -> Status {
    match error {
        ProjectionError::PoolExhausted(_)
        | ProjectionError::SigningKey(crate::signing_keys::SigningKeyError::PoolExhausted(_)) => {
            Status::unavailable("projection embedding database connection is unavailable")
        }
        ProjectionError::Database(_)
        | ProjectionError::EmbeddingsMigration(_)
        | ProjectionError::MalformedFact { .. }
        | ProjectionError::SigningKey(_) => {
            Status::internal("projection embedding storage operation failed")
        }
    }
}

#[tonic::async_trait]
impl v1::projection_embedding_service_server::ProjectionEmbeddingService
    for ProjectionEmbeddingService
{
    async fn recall_projected_nodes(
        &self,
        request: Request<v1::RecallProjectedNodesRequest>,
    ) -> Result<Response<v1::RecallProjectedNodesResult>, Status> {
        self.recall(request.into_inner()).await.map(Response::new)
    }

    async fn list_missing_projection_embeddings(
        &self,
        request: Request<v1::ListMissingProjectionEmbeddingsRequest>,
    ) -> Result<Response<v1::ListMissingProjectionEmbeddingsResult>, Status> {
        let request = request.into_inner();
        let operation = ProjectionEmbeddingOperation::ListMissing {
            model: &request.model,
            limit: request.limit,
        };
        operation.validate().map_err(Status::invalid_argument)?;
        self.authenticate(
            &request.tenant_id,
            &request.repository_id,
            &operation,
            request.authentication.as_ref(),
        )
        .await?;
        let limit = if request.limit == 0 {
            DEFAULT_EMBEDDING_LIST_LIMIT
        } else {
            request.limit
        };
        let nodes = self
            .projector
            .nodes_missing_embedding(
                &request.tenant_id,
                &request.repository_id,
                &request.model,
                i64::from(limit) + 1,
            )
            .await
            .map_err(store_error)?;
        let mut response = v1::ListMissingProjectionEmbeddingsResult::default();
        for node in nodes {
            if response.nodes.len() == limit as usize {
                response.has_more = true;
                break;
            }
            let source = v1::ProjectionEmbeddingSource {
                node_id: node.node_id,
                label: node.label,
            };
            source.validate().map_err(Status::failed_precondition)?;
            response.nodes.push(source);
            if response.encoded_len() + 2 > MAX_SOURCE_PAGE_BYTES {
                response.nodes.pop();
                response.has_more = true;
                break;
            }
        }
        Ok(Response::new(response))
    }

    async fn publish_projection_embedding(
        &self,
        request: Request<v1::PublishProjectionEmbeddingRequest>,
    ) -> Result<Response<v1::PublishProjectionEmbeddingResult>, Status> {
        let request = request.into_inner();
        let source = request
            .source
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("source is required"))?;
        let operation = ProjectionEmbeddingOperation::Publish {
            source,
            model: &request.model,
            embedding: &request.embedding,
        };
        operation.validate().map_err(Status::invalid_argument)?;
        self.authenticate(
            &request.tenant_id,
            &request.repository_id,
            &operation,
            request.authentication.as_ref(),
        )
        .await?;
        let stored = self
            .projector
            .upsert_embedding(
                &request.tenant_id,
                &request.repository_id,
                &UnembeddedNode {
                    node_id: source.node_id.clone(),
                    label: source.label.clone(),
                },
                &request.model,
                &request.embedding,
            )
            .await
            .map_err(store_error)?;
        Ok(Response::new(v1::PublishProjectionEmbeddingResult {
            stored,
        }))
    }
}

#[cfg(test)]
mod tests;

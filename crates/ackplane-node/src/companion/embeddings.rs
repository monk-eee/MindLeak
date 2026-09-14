use ackplane_client::{
    companion::wire::NodeReply, connect_channel, ClaimSigner, ClientError, SigningError,
};
use ackplane_protocol::{
    projection_embedding_auth::{projection_embedding_signing_bytes, ProjectionEmbeddingOperation},
    v1::{self, projection_embedding_service_client::ProjectionEmbeddingServiceClient},
};
use prost::Message;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

use super::{NodeService, ServiceSigner};

impl NodeService {
    fn projection_embedding_auth(
        &self,
        operation: ProjectionEmbeddingOperation<'_>,
    ) -> Result<v1::ProjectionEmbeddingAuthentication, ClientError> {
        operation.validate().map_err(|_| SigningError::Refused)?;
        let mut nonce = vec![0; 16];
        getrandom::getrandom(&mut nonce).map_err(|_| SigningError::Unavailable)?;
        let mut authentication = v1::ProjectionEmbeddingAuthentication {
            signing_key_id: self.binding.key_id.clone(),
            node_id: self.binding.node_id.clone(),
            signed_at: OffsetDateTime::now_utc()
                .format(&Rfc3339)
                .map_err(|_| SigningError::Refused)?,
            nonce,
            signature: Vec::new(),
        };
        authentication.signature =
            ServiceSigner(self).sign(&projection_embedding_signing_bytes(
                &self.binding.tenant_id,
                &self.binding.repository_id,
                &operation,
                &authentication,
            ))?;
        Ok(authentication)
    }

    fn missing_projection_embeddings_request(
        &self,
        model: String,
        limit: u32,
    ) -> Result<v1::ListMissingProjectionEmbeddingsRequest, ClientError> {
        let authentication =
            self.projection_embedding_auth(ProjectionEmbeddingOperation::ListMissing {
                model: &model,
                limit,
            })?;
        Ok(v1::ListMissingProjectionEmbeddingsRequest {
            tenant_id: self.binding.tenant_id.clone(),
            repository_id: self.binding.repository_id.clone(),
            model,
            limit,
            authentication: Some(authentication),
        })
    }

    fn publish_projection_embedding_request(
        &self,
        bytes: &[u8],
        model: String,
        embedding: Vec<f32>,
    ) -> Result<v1::PublishProjectionEmbeddingRequest, ClientError> {
        let source =
            v1::ProjectionEmbeddingSource::decode(bytes).map_err(|_| SigningError::Refused)?;
        if source.encode_to_vec() != bytes {
            return Err(SigningError::Refused.into());
        }
        let authentication =
            self.projection_embedding_auth(ProjectionEmbeddingOperation::Publish {
                source: &source,
                model: &model,
                embedding: &embedding,
            })?;
        Ok(v1::PublishProjectionEmbeddingRequest {
            tenant_id: self.binding.tenant_id.clone(),
            repository_id: self.binding.repository_id.clone(),
            source: Some(source),
            model,
            embedding,
            authentication: Some(authentication),
        })
    }

    pub(super) async fn missing_projection_embeddings(
        &self,
        model: String,
        limit: u32,
    ) -> Result<NodeReply, ClientError> {
        let request = self.missing_projection_embeddings_request(model, limit)?;
        let result = ProjectionEmbeddingServiceClient::new(connect_channel(&self.endpoint).await?)
            .list_missing_projection_embeddings(request)
            .await?
            .into_inner();
        Ok(NodeReply::Payload(result.encode_to_vec()))
    }

    pub(super) async fn publish_projection_embedding(
        &self,
        source: &[u8],
        model: String,
        embedding: Vec<f32>,
    ) -> Result<NodeReply, ClientError> {
        let request = self.publish_projection_embedding_request(source, model, embedding)?;
        let result = ProjectionEmbeddingServiceClient::new(connect_channel(&self.endpoint).await?)
            .publish_projection_embedding(request)
            .await?
            .into_inner();
        Ok(NodeReply::Payload(result.encode_to_vec()))
    }

    pub(super) async fn recall_projected_nodes(
        &self,
        model: String,
        query_embedding: Vec<f32>,
        floor: f32,
        limit: u32,
    ) -> Result<NodeReply, ClientError> {
        let authentication =
            self.projection_embedding_auth(ProjectionEmbeddingOperation::Recall {
                model: &model,
                query_embedding: &query_embedding,
                floor,
                limit,
            })?;
        let request = v1::RecallProjectedNodesRequest {
            tenant_id: self.binding.tenant_id.clone(),
            repository_id: self.binding.repository_id.clone(),
            model,
            query_embedding,
            floor,
            limit,
            authentication: Some(authentication),
        };
        let result = ProjectionEmbeddingServiceClient::new(connect_channel(&self.endpoint).await?)
            .recall_projected_nodes(request)
            .await?
            .into_inner();
        Ok(NodeReply::Payload(result.encode_to_vec()))
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod recall_tests;

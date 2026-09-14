use ackplane_protocol::{projection_embedding_auth::ProjectionEmbeddingOperation, v1};
use prost::Message;
use tonic::Status;

use super::{store_error, ProjectionEmbeddingService, MAX_SOURCE_PAGE_BYTES};
use crate::projection::RecallState;

impl ProjectionEmbeddingService {
    pub(super) async fn recall(
        &self,
        request: v1::RecallProjectedNodesRequest,
    ) -> Result<v1::RecallProjectedNodesResult, Status> {
        let operation = ProjectionEmbeddingOperation::Recall {
            model: &request.model,
            query_embedding: &request.query_embedding,
            floor: request.floor,
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
            10
        } else {
            request.limit
        };
        let snapshot = self
            .projector
            .recall(
                &request.tenant_id,
                &request.repository_id,
                &request.model,
                &request.query_embedding,
                request.floor,
                limit as usize,
            )
            .await
            .map_err(store_error)?;
        let state = match snapshot.state() {
            RecallState::Empty => v1::ProjectionRecallState::Empty,
            RecallState::NotYetProjected => v1::ProjectionRecallState::NotYetProjected,
            RecallState::Stale => v1::ProjectionRecallState::Stale,
            RecallState::NotYetEmbedded => v1::ProjectionRecallState::NotYetEmbedded,
            RecallState::PartiallyEmbedded => v1::ProjectionRecallState::PartiallyEmbedded,
            RecallState::Current => v1::ProjectionRecallState::Current,
        };
        let mut response = v1::RecallProjectedNodesResult {
            hits: vec![],
            state: state as i32,
            projected_nodes: snapshot
                .projected_nodes
                .try_into()
                .map_err(|_| Status::internal("invalid projection coverage"))?,
            embedded_nodes: snapshot
                .embedded_nodes
                .try_into()
                .map_err(|_| Status::internal("invalid embedding coverage"))?,
            ledger_position: snapshot.ledger_position,
            projected_position: snapshot.projected_position,
            projected_at: snapshot
                .projected_at
                .map(crate::wire_format::rfc3339)
                .transpose()
                .map_err(|_| Status::internal("invalid projection timestamp"))?,
            searched: snapshot.searched,
            truncated: false,
            model: request.model,
        };
        for hit in snapshot.nodes {
            response.hits.push(v1::ProjectionRecallHit {
                node_id: hit.node_id,
                label: hit.label,
                node_type: hit.node_type,
                similarity: hit.similarity,
            });
            if response.encoded_len() + 2 > MAX_SOURCE_PAGE_BYTES {
                response.hits.pop();
                if response.hits.is_empty() {
                    return Err(Status::failed_precondition(
                        "a recall hit exceeds the response limit",
                    ));
                }
                response.truncated = true;
                break;
            }
        }
        Ok(response)
    }
}

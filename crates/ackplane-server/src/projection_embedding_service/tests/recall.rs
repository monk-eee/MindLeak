use ackplane_protocol::{
    projection_embedding_auth::ProjectionEmbeddingOperation,
    v1::{ProjectionRecallState, RecallProjectedNodesRequest},
};

use super::*;

mod authentication;
mod isolation;
mod lifecycle;
mod ranking;
mod response_bounds;

fn unit_vector(first_component: f32) -> Vec<f32> {
    let mut vector = vec![0.0; 768];
    vector[0] = first_component;
    vector[1] = (1.0 - first_component * first_component).sqrt();
    vector
}

impl Fixture {
    fn recall_request(
        &self,
        model: &str,
        query_embedding: &[f32],
        floor: f32,
        limit: u32,
    ) -> RecallProjectedNodesRequest {
        RecallProjectedNodesRequest {
            tenant_id: self.binding.tenant_id.clone(),
            repository_id: self.binding.repository_id.clone(),
            model: model.into(),
            query_embedding: query_embedding.to_vec(),
            floor,
            limit,
            authentication: Some(self.authentication(ProjectionEmbeddingOperation::Recall {
                model,
                query_embedding,
                floor,
                limit,
            })),
        }
    }
}

#[tokio::test]
async fn empty_probe_distinguishes_absent_projection_from_zero_checkpoint() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let mut client = fixture.server.client().await;

    for rebuilt in [false, true] {
        if rebuilt {
            fixture.project(&[]).await;
        }
        let response = client
            .recall_projected_nodes(fixture.recall_request("recall-model", &[], 0.0, 0))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(response.state(), ProjectionRecallState::Empty);
        assert_eq!(response.projected_nodes, 0);
        assert_eq!(response.embedded_nodes, 0);
        assert_eq!(response.ledger_position, 0);
        assert_eq!(response.projected_position, rebuilt.then_some(0));
        assert_eq!(response.projected_at.is_some(), rebuilt);
        if let Some(projected_at) = response.projected_at.as_deref() {
            OffsetDateTime::parse(projected_at, &Rfc3339).unwrap();
        }
        assert!(response.hits.is_empty());
        assert!(!response.searched);
        assert!(!response.truncated);
        assert_eq!(response.model, "recall-model");
    }

    drop(client);
    fixture.server.stop().await;
}

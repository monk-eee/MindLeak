use ackplane_client::companion::wire::Operation;
use ed25519_dalek::{Signature, VerifyingKey};

use super::tests::service;
use super::*;

#[test]
fn recall_auth_binds_query_floor_limit_and_model() {
    let service = service();
    let query_embedding = vec![0.25; 768];
    let mut changed_query = query_embedding.clone();
    changed_query[767] = 0.75;
    let operation = ProjectionEmbeddingOperation::Recall {
        model: "model",
        query_embedding: &query_embedding,
        floor: 0.5,
        limit: 10,
    };
    let authentication = service.projection_embedding_auth(operation).unwrap();
    let public_key = VerifyingKey::from_bytes(&service.signer.identity().public_key).unwrap();
    let signature = Signature::from_slice(&authentication.signature).unwrap();
    let bytes = projection_embedding_signing_bytes(
        &service.binding.tenant_id,
        &service.binding.repository_id,
        &operation,
        &authentication,
    );
    public_key.verify_strict(&bytes, &signature).unwrap();
    for (model, query_embedding, floor, limit) in [
        ("model", changed_query.as_slice(), 0.5, 10),
        ("model", query_embedding.as_slice(), 0.75, 10),
        ("model", query_embedding.as_slice(), 0.5, 11),
        ("other-model", query_embedding.as_slice(), 0.5, 10),
    ] {
        let changed = ProjectionEmbeddingOperation::Recall {
            model,
            query_embedding,
            floor,
            limit,
        };
        let bytes = projection_embedding_signing_bytes(
            &service.binding.tenant_id,
            &service.binding.repository_id,
            &changed,
            &authentication,
        );
        assert!(public_key.verify_strict(&bytes, &signature).is_err());
    }
}

#[test]
fn recall_auth_accepts_status_probes_and_query_option_boundaries() {
    let service = service();
    for query_embedding in [vec![], vec![0.25; 768]] {
        for floor in [0.0, 1.0] {
            for limit in [0, 10, 100] {
                service
                    .projection_embedding_auth(ProjectionEmbeddingOperation::Recall {
                        model: "model",
                        query_embedding: &query_embedding,
                        floor,
                        limit,
                    })
                    .unwrap();
            }
        }
    }
}

#[tokio::test]
async fn dispatch_refuses_invalid_recall_payloads_before_connecting() {
    let service = service();
    for (query_embedding, floor, limit) in [
        (vec![1.0; 767], 0.5, 10),
        (vec![1.0; 769], 0.5, 10),
        (vec![0.0; 768], 0.5, 10),
        (vec![f32::NAN; 768], 0.5, 10),
        (vec![1.0; 768], f32::NAN, 10),
        (vec![], f32::NAN, 10),
        (vec![], -0.1, 10),
        (vec![], 1.1, 10),
        (vec![1.0; 768], 0.5, 101),
        (vec![], 0.5, 101),
    ] {
        assert!(matches!(
            service
                .dispatch(Operation::ProjectionRecall {
                    model: "model".into(),
                    query_embedding,
                    floor,
                    limit,
                })
                .await,
            Err(ClientError::Signing(SigningError::Refused))
        ));
    }
}

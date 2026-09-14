use std::{future::Future, time::Duration};

use serde_json::json;

#[path = "../../ackplane-node/tests/support/companion.rs"]
mod companion;
#[path = "indexing/enrollment.rs"]
mod enrollment;
#[path = "indexing/failures.rs"]
mod failures;
#[path = "indexing/fixture.rs"]
mod fixture;
#[path = "indexing/model.rs"]
mod model;
#[path = "indexing/process.rs"]
mod process;
#[path = "indexing/service.rs"]
mod service;

const MODEL: &str = "industrial-index-test-model";
const API_KEY: &str = "index-test-only-api-key-sentinel";

async fn bounded(test: impl Future<Output = ()>) {
    tokio::time::timeout(Duration::from_secs(45), test)
        .await
        .expect("the indexing end-to-end case must finish within 45 seconds");
}

#[tokio::test]
async fn real_stdio_indexes_labels_with_reversed_indices_and_skips_existing_vectors() {
    bounded(async {
        let Some(fixture) = fixture::Fixture::new(2).await else {
            return;
        };
        let model = model::ModelServer::start(model::Mode::Valid).await;
        let replies = fixture
            .index(&model.url, MODEL, &[json!({}), json!({})])
            .await;
        for (reply, indexed) in replies.iter().zip([2, 0]) {
            assert!(!reply.is_error);
            assert_eq!(
                reply.progress,
                json!({
                    "model": MODEL, "status": "complete", "indexed": indexed,
                    "attempted": indexed, "stale_sources": 0, "remaining": false
                })
            );
        }
        let requests = model.requests();
        assert_eq!(
            requests.len(),
            1,
            "the second index must not call the model"
        );
        assert_eq!(
            requests[0].body,
            json!({
                "model": MODEL, "input": fixture.labels.values().collect::<Vec<_>>()
            })
        );
        assert_eq!(requests[0].method, axum::http::Method::POST);
        assert!(requests[0].authorization.as_deref() == Some(&format!("Bearer {API_KEY}")));
        let stored = fixture.embeddings().await;
        assert_eq!(stored.len(), 2);
        assert_ne!(stored[0].2, stored[1].2);
        for (node_id, model_name, vector) in stored {
            assert_eq!(model_name, MODEL);
            assert_eq!(vector, model::vector_for_label(&fixture.labels[&node_id]));
        }
        model.stop().await;
        fixture.stop().await;
    })
    .await;
}

#[tokio::test]
async fn real_stdio_respects_limit_and_finishes_across_twenty_source_pages() {
    bounded(async {
        let Some(fixture) = fixture::Fixture::new(24).await else {
            return;
        };
        let model = model::ModelServer::start(model::Mode::Valid).await;
        let replies = fixture
            .index(
                &model.url,
                MODEL,
                &[json!({"limit": 3}), json!({"limit": 50}), json!({})],
            )
            .await;
        for (reply, (status, indexed, remaining)) in replies.iter().zip([
            ("limited", 3, true),
            ("complete", 21, false),
            ("complete", 0, false),
        ]) {
            assert!(!reply.is_error);
            assert_eq!(
                reply.progress,
                json!({
                    "model": MODEL, "status": status, "indexed": indexed,
                    "attempted": indexed, "stale_sources": 0, "remaining": remaining,
                })
            );
        }
        let requests = model.requests();
        assert_eq!(
            requests.len(),
            3,
            "the final no-op pass must make no model request"
        );
        let mut inputs = Vec::new();
        for (request, batch_size) in requests.iter().zip([3, 20, 1]) {
            assert_eq!(request.body.as_object().unwrap().len(), 2);
            assert_eq!(request.body["model"], MODEL);
            let batch = request.body["input"].as_array().unwrap();
            assert_eq!(batch.len(), batch_size);
            assert!(batch.len() <= 20);
            inputs.extend(
                batch
                    .iter()
                    .map(|label| label.as_str().unwrap().to_string()),
            );
        }
        inputs.sort();
        assert_eq!(inputs, fixture.labels.values().cloned().collect::<Vec<_>>());
        assert_eq!(fixture.embeddings().await.len(), 24);
        model.stop().await;
        fixture.stop().await;
    })
    .await;
}

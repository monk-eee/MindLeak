use std::collections::BTreeMap;

use serde_json::json;

use super::{
    bounded,
    companion::TestCompanion,
    fixture::Fixture,
    model::{vector_for_label, Mode, ModelServer},
    API_KEY, MODEL,
};

#[path = "recall/queries.rs"]
mod queries;
#[path = "recall/states.rs"]
mod states;

#[tokio::test]
async fn recall_independently_enrolled_consumer_reads_the_same_postgres_vectors() {
    bounded(async {
        let Some(fixture) = Fixture::new(2).await else {
            return;
        };
        let query = "independent consumer query only";
        let (node_id, label) = fixture.labels.first_key_value().unwrap();
        let query_vector = vector_for_label(label);
        let mut vectors: BTreeMap<_, _> = fixture
            .labels
            .values()
            .map(|label| (label.clone(), vector_for_label(label)))
            .collect();
        vectors.insert(query.into(), query_vector.clone());
        let model = ModelServer::start(Mode::Vectors(vectors.clone())).await;
        let indexed = fixture.index(&model.url, MODEL, &[json!({})]).await;
        assert!(!indexed[0].is_error);
        assert_eq!(indexed[0].progress["indexed"], 2);
        let stored = fixture.embeddings().await;
        assert_eq!(stored.len(), 2);
        for (stored_id, stored_model, stored_vector) in &stored {
            assert_eq!(stored_model, MODEL);
            assert_eq!(stored_vector, &vectors[&fixture.labels[stored_id]]);
        }

        let arguments = [json!({"query": query, "limit": 1})];
        let producer = fixture.recall(&model.url, MODEL, &arguments).await;
        let consumer_seed = [114; 32];
        let consumer_binding = fixture.enroll(&consumer_seed).await;
        assert_eq!(consumer_binding.tenant_id, fixture.binding.tenant_id);
        assert_eq!(
            consumer_binding.repository_id,
            fixture.binding.repository_id
        );
        assert_ne!(consumer_binding.node_id, fixture.binding.node_id);
        assert_ne!(consumer_binding.key_id, fixture.binding.key_id);
        let consumer_directory = tempfile::Builder::new().prefix("rc-").tempdir().unwrap();
        assert_ne!(consumer_directory.path(), fixture.directory.path());
        let consumer = TestCompanion::start(
            &fixture.endpoint,
            consumer_binding,
            &consumer_seed,
            consumer_directory.path(),
        )
        .await;
        let consumed = fixture
            .recall_from(consumer_directory.path(), &model.url, MODEL, &arguments)
            .await;
        for reply in [&producer[0], &consumed[0]] {
            assert!(!reply.is_error);
            assert_eq!(reply.progress["state"], "current");
            assert_eq!(reply.progress["model"], MODEL);
            assert_eq!(reply.progress["searched"], true);
            assert_eq!(
                reply.progress["coverage"],
                json!({"projected_nodes": 2, "embedded_nodes": 2})
            );
            let results = reply.progress["results"].as_array().unwrap();
            assert_eq!(results.len(), 1);
            assert_eq!(results[0]["id"], *node_id);
            assert_eq!(results[0]["label"], *label);
            assert_eq!(results[0]["node_type"], "artifact");
            let score = results[0]["score"].as_f64().unwrap();
            assert!(score.is_finite());
            assert!((score - 1.0).abs() < 0.00001);
            assert_eq!(reply.progress["truncated"], false);
        }
        assert_eq!(producer[0].progress, consumed[0].progress);
        let requests = model.requests();
        assert_eq!(
            requests.len(),
            3,
            "one index batch and one query per consumer"
        );
        assert_eq!(
            requests[0].body,
            json!({
                "model": MODEL, "input": fixture.labels.values().collect::<Vec<_>>()
            })
        );
        for request in &requests[1..] {
            assert_eq!(request.body, json!({"model": MODEL, "input": [query]}));
            assert_eq!(request.method, axum::http::Method::POST);
            assert!(request.authorization.as_deref() == Some(&format!("Bearer {API_KEY}")));
        }
        drop(consumer);
        model.stop().await;
        fixture.stop().await;
    })
    .await;
}

#[tokio::test]
async fn recall_unembedded_projection_does_not_request_a_query_embedding() {
    bounded(async {
        let Some(fixture) = Fixture::new(2).await else {
            return;
        };
        let model = ModelServer::start(Mode::Valid).await;
        let replies = fixture
            .recall(&model.url, MODEL, &[json!({"query": "unindexed query"})])
            .await;
        let reply = &replies[0];
        assert!(!reply.is_error);
        assert_eq!(reply.progress["state"], "not_yet_embedded");
        assert_eq!(reply.progress["model"], MODEL);
        assert_eq!(reply.progress["searched"], false);
        assert_eq!(reply.progress["results"], json!([]));
        assert_eq!(
            reply.progress["coverage"],
            json!({"projected_nodes": 2, "embedded_nodes": 0})
        );
        assert_eq!(reply.progress["truncated"], false);
        assert!(model.requests().is_empty());
        model.stop().await;
        fixture.stop().await;
    })
    .await;
}

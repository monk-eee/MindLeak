use std::collections::BTreeMap;

use serde_json::json;

use super::{bounded, Fixture, Mode, ModelServer, API_KEY, MODEL};

#[tokio::test]
async fn recall_orthogonal_query_is_current_no_match_and_matching_query_returns_raw_cosine() {
    bounded(async {
        let Some(fixture) = Fixture::new(2).await else {
            return;
        };
        let labels: Vec<_> = fixture.labels.values().cloned().collect();
        let mut matching_vector = vec![0.0; 768];
        matching_vector[0] = 1.0;
        let mut nearby_vector = vec![0.0; 768];
        nearby_vector[0] = 0.9;
        nearby_vector[1] = 0.1;
        let mut orthogonal_vector = vec![0.0; 768];
        orthogonal_vector[1] = 1.0;
        let orthogonal_query = "orthogonal query only";
        let matching_query = "matching query only";
        let model = ModelServer::start(Mode::Vectors(BTreeMap::from([
            (labels[0].clone(), matching_vector.clone()),
            (labels[1].clone(), nearby_vector),
            (orthogonal_query.into(), orthogonal_vector),
            (matching_query.into(), matching_vector),
        ])))
        .await;
        let indexed = fixture.index(&model.url, MODEL, &[json!({})]).await;
        assert!(!indexed[0].is_error);
        assert_eq!(indexed[0].progress["indexed"], 2);
        let stored = fixture.embeddings().await;
        assert_eq!(stored.len(), 2);
        let replies = fixture
            .recall(
                &model.url,
                MODEL,
                &[
                    json!({"query": orthogonal_query}),
                    json!({"query": matching_query}),
                ],
            )
            .await;
        for reply in &replies {
            assert!(!reply.is_error);
            assert_eq!(reply.progress["state"], "current");
            assert_eq!(reply.progress["model"], MODEL);
            assert_eq!(reply.progress["searched"], true);
            assert_eq!(
                reply.progress["coverage"],
                json!({"projected_nodes": 2, "embedded_nodes": 2})
            );
            let projection = &reply.progress["projection"];
            assert_eq!(
                projection["projected_position"],
                projection["ledger_position"]
            );
            assert!(projection["projected_position"].as_i64().unwrap() > 0);
            assert_eq!(reply.progress["truncated"], false);
        }
        assert_eq!(replies[0].progress["results"], json!([]));
        assert_eq!(
            replies[0].progress["projection"],
            replies[1].progress["projection"]
        );
        let results = replies[1].progress["results"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        let expected_scores = [1.0, 0.9_f64 / (0.9_f64.powi(2) + 0.1_f64.powi(2)).sqrt()];
        for ((result, (node_id, label)), expected_score) in
            results.iter().zip(&fixture.labels).zip(expected_scores)
        {
            assert_eq!(result["id"], *node_id);
            assert_eq!(result["label"], *label);
            assert_eq!(result["node_type"], "artifact");
            let score = result["score"].as_f64().unwrap();
            assert!(score.is_finite() && (score - expected_score).abs() < 0.00001);
        }
        let requests = model.requests();
        assert_eq!(
            requests.len(),
            3,
            "one index batch and two query embeddings"
        );
        assert_eq!(requests[0].body, json!({"model": MODEL, "input": labels}));
        for (request, query) in requests[1..].iter().zip([orthogonal_query, matching_query]) {
            assert_eq!(request.body, json!({"model": MODEL, "input": [query]}));
            assert_eq!(request.method, axum::http::Method::POST);
            assert!(request.authorization.as_deref() == Some(&format!("Bearer {API_KEY}")));
        }
        assert_eq!(fixture.embeddings().await, stored);
        model.stop().await;
        fixture.stop().await;
    })
    .await;
}

#[tokio::test]
async fn recall_model_failures_preserve_observed_coverage_without_search_or_mutation() {
    bounded(async {
        let Some(fixture) = Fixture::new(2).await else {
            return;
        };
        let index_model = ModelServer::start(Mode::Valid).await;
        let indexed = fixture.index(&index_model.url, MODEL, &[json!({})]).await;
        assert!(!indexed[0].is_error);
        assert_eq!(indexed[0].progress["indexed"], 2);
        assert_eq!(index_model.requests().len(), 1);
        let stored = fixture.embeddings().await;
        assert_eq!(stored.len(), 2);
        index_model.stop().await;

        let mut previous_observation = None;
        for (name, mode) in [
            ("HTTP 503", Mode::Unavailable),
            ("wrong dimensions", Mode::WrongDimensions),
            ("malformed index", Mode::MalformedIndex),
        ] {
            let model = ModelServer::start(mode).await;
            let query = format!("failing query for {name}");
            let replies = fixture
                .recall(&model.url, MODEL, &[json!({"query": query})])
                .await;
            let reply = &replies[0];
            assert!(reply.is_error, "{name} must be an MCP tool error");
            assert_eq!(reply.progress["state"], "unavailable", "{name}");
            assert_eq!(reply.progress["searched"], false, "{name}");
            assert_eq!(reply.progress["results"], json!([]), "{name}");
            assert_eq!(reply.progress["failure"]["stage"], "embedding", "{name}");
            assert!(!reply.progress["failure"]["message"]
                .as_str()
                .unwrap()
                .is_empty());
            let observed = &reply.progress["last_observed"];
            assert_eq!(observed["state"], "current", "{name}");
            assert_eq!(observed["model"], MODEL, "{name}");
            assert_eq!(observed["searched"], false, "{name}");
            assert_eq!(observed["results"], json!([]), "{name}");
            assert_eq!(
                observed["coverage"],
                json!({"projected_nodes": 2, "embedded_nodes": 2}),
                "{name}"
            );
            let projection = &observed["projection"];
            assert_eq!(
                projection["projected_position"],
                projection["ledger_position"]
            );
            assert!(projection["projected_position"].as_i64().unwrap() > 0);
            assert_eq!(observed["truncated"], false, "{name}");
            if let Some(previous) = &previous_observation {
                assert_eq!(
                    observed, previous,
                    "{name} must preserve the observed projection"
                );
            }
            previous_observation = Some(observed.clone());
            let requests = model.requests();
            assert_eq!(
                requests.len(),
                1,
                "{name} must request only one query embedding"
            );
            assert_eq!(requests[0].body, json!({"model": MODEL, "input": [query]}));
            assert_eq!(requests[0].method, axum::http::Method::POST);
            assert!(requests[0].authorization.as_deref() == Some(&format!("Bearer {API_KEY}")));
            assert_eq!(
                fixture.embeddings().await,
                stored,
                "{name} must not mutate vectors"
            );
            model.stop().await;
        }
        fixture.stop().await;
    })
    .await;
}

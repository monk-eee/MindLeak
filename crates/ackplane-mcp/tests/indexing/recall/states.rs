use std::collections::BTreeMap;

use ackplane_server::projection::StructuralFact;
use serde_json::json;

use super::{bounded, vector_for_label, Fixture, Mode, ModelServer, MODEL};

#[tokio::test]
async fn recall_empty_and_different_model_skip_query_embedding() {
    bounded(async {
        let Some(empty) = Fixture::new(0).await else {
            return;
        };
        let model = ModelServer::start(Mode::Valid).await;
        let replies = empty
            .recall(&model.url, MODEL, &[json!({"query": "empty corpus query"})])
            .await;
        assert!(!replies[0].is_error);
        assert_eq!(replies[0].progress["state"], "empty");
        assert_eq!(replies[0].progress["model"], MODEL);
        assert_eq!(replies[0].progress["searched"], false);
        assert_eq!(replies[0].progress["results"], json!([]));
        assert_eq!(
            replies[0].progress["coverage"],
            json!({"projected_nodes": 0, "embedded_nodes": 0})
        );
        assert_eq!(replies[0].progress["projection"]["ledger_position"], 0);
        assert_eq!(replies[0].progress["projection"]["projected_position"], 0);
        assert_eq!(replies[0].progress["truncated"], false);
        assert!(model.requests().is_empty());
        assert!(empty.embeddings().await.is_empty());
        empty.stop().await;

        let fixture = Fixture::new(2).await.unwrap();
        let indexed = fixture.index(&model.url, MODEL, &[json!({})]).await;
        assert!(!indexed[0].is_error);
        assert_eq!(indexed[0].progress["indexed"], 2);
        let stored = fixture.embeddings().await;
        assert_eq!(stored.len(), 2);
        let different_model = "different-recall-model";
        let replies = fixture
            .recall(
                &model.url,
                different_model,
                &[json!({"query": "different model query"})],
            )
            .await;
        assert!(!replies[0].is_error);
        assert_eq!(replies[0].progress["state"], "not_yet_embedded");
        assert_eq!(replies[0].progress["model"], different_model);
        assert_eq!(replies[0].progress["searched"], false);
        assert_eq!(replies[0].progress["results"], json!([]));
        assert_eq!(
            replies[0].progress["coverage"],
            json!({"projected_nodes": 2, "embedded_nodes": 0})
        );
        let projection = &replies[0].progress["projection"];
        assert_eq!(
            projection["projected_position"],
            projection["ledger_position"]
        );
        assert!(projection["projected_position"].as_i64().unwrap() > 0);
        assert_eq!(replies[0].progress["truncated"], false);
        let requests = model.requests();
        assert_eq!(requests.len(), 1, "only indexing may request embeddings");
        assert_eq!(
            requests[0].body,
            json!({"model": MODEL, "input": fixture.labels.values().collect::<Vec<_>>()})
        );
        assert_eq!(fixture.embeddings().await, stored);
        model.stop().await;
        fixture.stop().await;
    })
    .await;
}

#[tokio::test]
async fn recall_stale_then_rebuilt_requires_reindexing_before_search() {
    bounded(async {
        let Some(mut fixture) = Fixture::new(1).await else {
            return;
        };
        let (node_id, original_label) = fixture.labels.first_key_value().unwrap();
        let node_id = node_id.clone();
        let original_label = original_label.clone();
        let changed_label = format!("{original_label} changed after indexing");
        let query = "updated source query";
        let changed_vector = vector_for_label(&changed_label);
        let vectors = BTreeMap::from([
            (original_label.clone(), vector_for_label(&original_label)),
            (changed_label.clone(), changed_vector.clone()),
            (query.into(), changed_vector.clone()),
        ]);
        let model = ModelServer::start(Mode::Vectors(vectors)).await;
        let indexed = fixture.index(&model.url, MODEL, &[json!({})]).await;
        assert!(!indexed[0].is_error);
        assert_eq!(indexed[0].progress["indexed"], 1);
        let original_embeddings = fixture.embeddings().await;
        assert_eq!(original_embeddings.len(), 1);

        fixture
            .append_fact(StructuralFact {
                node_id: node_id.clone(),
                node_type: "artifact".into(),
                label: changed_label.clone(),
                edges: vec![],
            })
            .await;
        let arguments = [json!({"query": query})];
        let stale = fixture.recall(&model.url, MODEL, &arguments).await;
        assert!(!stale[0].is_error);
        assert_eq!(stale[0].progress["state"], "stale");
        assert_eq!(stale[0].progress["model"], MODEL);
        assert_eq!(stale[0].progress["searched"], false);
        assert_eq!(stale[0].progress["results"], json!([]));
        assert_eq!(
            stale[0].progress["coverage"],
            json!({"projected_nodes": 1, "embedded_nodes": 1})
        );
        let projection = &stale[0].progress["projection"];
        let checkpoint = projection["projected_position"].as_i64().unwrap();
        let ledger_position = projection["ledger_position"].as_i64().unwrap();
        assert!(checkpoint > 0 && checkpoint < ledger_position);
        assert_eq!(stale[0].progress["truncated"], false);
        assert_eq!(model.requests().len(), 1, "stale recall must not call HTTP");
        assert_eq!(fixture.embeddings().await, original_embeddings);

        fixture.rebuild().await;
        let rebuilt = fixture.recall(&model.url, MODEL, &arguments).await;
        assert!(!rebuilt[0].is_error);
        assert_eq!(rebuilt[0].progress["state"], "not_yet_embedded");
        assert_eq!(rebuilt[0].progress["model"], MODEL);
        assert_eq!(rebuilt[0].progress["searched"], false);
        assert_eq!(rebuilt[0].progress["results"], json!([]));
        assert_eq!(
            rebuilt[0].progress["coverage"],
            json!({"projected_nodes": 1, "embedded_nodes": 0})
        );
        let projection = &rebuilt[0].progress["projection"];
        assert_eq!(projection["ledger_position"], ledger_position);
        assert_eq!(projection["projected_position"], ledger_position);
        assert_eq!(rebuilt[0].progress["truncated"], false);
        assert!(fixture.embeddings().await.is_empty());
        assert_eq!(
            model.requests().len(),
            1,
            "rebuilt recall must not call HTTP"
        );

        let indexed = fixture.index(&model.url, MODEL, &[json!({})]).await;
        assert!(!indexed[0].is_error);
        assert_eq!(indexed[0].progress["indexed"], 1);
        assert_eq!(
            fixture.embeddings().await,
            vec![(node_id.clone(), MODEL.into(), changed_vector)]
        );
        let current = fixture.recall(&model.url, MODEL, &arguments).await;
        assert!(!current[0].is_error);
        assert_eq!(current[0].progress["state"], "current");
        assert_eq!(current[0].progress["model"], MODEL);
        assert_eq!(current[0].progress["searched"], true);
        assert_eq!(
            current[0].progress["coverage"],
            json!({"projected_nodes": 1, "embedded_nodes": 1})
        );
        assert_eq!(
            current[0].progress["projection"],
            rebuilt[0].progress["projection"]
        );
        let results = current[0].progress["results"].as_array().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["id"], node_id);
        assert_eq!(results[0]["label"], changed_label);
        assert_eq!(results[0]["node_type"], "artifact");
        let score = results[0]["score"].as_f64().unwrap();
        assert!(score.is_finite() && (score - 1.0).abs() < 0.00001);
        assert_eq!(current[0].progress["truncated"], false);
        let requests = model.requests();
        assert_eq!(
            requests.len(),
            3,
            "two index batches and one query embedding"
        );
        for (request, input) in
            requests
                .iter()
                .zip([original_label.as_str(), &changed_label, query])
        {
            assert_eq!(request.body, json!({"model": MODEL, "input": [input]}));
        }
        model.stop().await;
        fixture.stop().await;
    })
    .await;
}

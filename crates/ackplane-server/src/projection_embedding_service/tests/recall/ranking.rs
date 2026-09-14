use crate::projection::StructuralFact;

use super::*;

#[tokio::test]
async fn real_pgvector_ranking_rejects_flat_fields_and_preserves_raw_scores_under_kind_priors() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let model = "recall-model";
    let query = unit_vector(1.0);
    let mut facts: Vec<_> = (0..8)
        .map(|index| StructuralFact {
            node_id: format!("artifact:background-{index}.rs"),
            node_type: "artifact".into(),
            label: format!("background {index}"),
            edges: vec![],
        })
        .collect();
    facts.extend([
        StructuralFact {
            node_id: "symbol:parse_imports".into(),
            node_type: "symbol".into(),
            label: "parse_imports".into(),
            edges: vec![],
        },
        StructuralFact {
            node_id: "intent:why-we-parse".into(),
            node_type: "intent".into(),
            label: "why imports are parsed".into(),
            edges: vec![],
        },
    ]);
    fixture
        .append_facts_in(
            &fixture.binding.tenant_id,
            &fixture.binding.repository_id,
            &facts,
        )
        .await;
    let summary = fixture
        .projector
        .rebuild(&fixture.binding.tenant_id, &fixture.binding.repository_id)
        .await
        .unwrap();
    assert_eq!(summary.nodes, 10);
    let sources: Vec<_> = facts
        .iter()
        .map(|fact| ProjectionEmbeddingSource {
            node_id: fact.node_id.clone(),
            label: fact.label.clone(),
        })
        .collect();
    let mut client = fixture.server.client().await;
    let flat_vector = unit_vector(0.54);
    for source in &sources {
        assert!(
            client
                .publish_projection_embedding(fixture.publish(source, model, &flat_vector))
                .await
                .unwrap()
                .into_inner()
                .stored
        );
    }
    let flat = client
        .recall_projected_nodes(fixture.recall_request(model, &query, 0.5, 100))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(flat.state(), ProjectionRecallState::Current);
    assert_eq!(flat.projected_nodes, 10);
    assert_eq!(flat.embedded_nodes, 10);
    assert_eq!(flat.projected_position, Some(summary.stream_position));
    assert!(flat.searched);
    assert!(!flat.truncated);
    assert!(
        flat.hits.is_empty(),
        "a flat field above the floor is not an answer: {flat:?}"
    );

    assert!(
        client
            .publish_projection_embedding(fixture.publish(&sources[8], model, &unit_vector(0.95)))
            .await
            .unwrap()
            .into_inner()
            .stored
    );
    let standout = client
        .recall_projected_nodes(fixture.recall_request(model, &query, 0.5, 100))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(standout.state(), ProjectionRecallState::Current);
    assert!(standout.searched);
    assert_eq!(standout.hits.len(), 1);
    assert_eq!(standout.hits[0].node_id, sources[8].node_id);
    assert_eq!(standout.hits[0].label, sources[8].label);
    assert_eq!(standout.hits[0].node_type, "symbol");
    assert!(
        (standout.hits[0].similarity - 0.95).abs() < 1e-6,
        "{standout:?}"
    );

    for (index, source) in sources.iter().enumerate() {
        let similarity = match index {
            8 => 0.81,
            9 => 0.80,
            _ => 0.10,
        };
        assert!(
            client
                .publish_projection_embedding(fixture.publish(
                    source,
                    model,
                    &unit_vector(similarity)
                ))
                .await
                .unwrap()
                .into_inner()
                .stored
        );
    }
    let tied = client
        .recall_projected_nodes(fixture.recall_request(model, &query, 0.0, 0))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(tied.state(), ProjectionRecallState::Current);
    assert_eq!(tied.projected_nodes, 10);
    assert_eq!(tied.embedded_nodes, 10);
    assert_eq!(tied.projected_position, flat.projected_position);
    assert_eq!(tied.projected_at, flat.projected_at);
    assert!(tied.searched);
    assert!(!tied.truncated);
    assert_eq!(tied.hits.len(), 2);
    assert_eq!(tied.hits[0].node_id, sources[9].node_id);
    assert_eq!(tied.hits[0].label, sources[9].label);
    assert_eq!(tied.hits[0].node_type, "intent");
    assert!((tied.hits[0].similarity - 0.80).abs() < 1e-6, "{tied:?}");
    assert_eq!(tied.hits[1].node_id, sources[8].node_id);
    assert_eq!(tied.hits[1].label, sources[8].label);
    assert_eq!(tied.hits[1].node_type, "symbol");
    assert!((tied.hits[1].similarity - 0.81).abs() < 1e-6, "{tied:?}");
    assert!(tied.hits[0].similarity < tied.hits[1].similarity);

    let limited = client
        .recall_projected_nodes(fixture.recall_request(model, &query, 0.0, 1))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(limited.hits, tied.hits[..1]);
    assert!(!limited.truncated);
    let floored = client
        .recall_projected_nodes(fixture.recall_request(model, &query, 0.805, 10))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(floored.hits, tied.hits[1..]);
    assert!(floored.searched);
    let above_all = client
        .recall_projected_nodes(fixture.recall_request(model, &query, 1.0, 10))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(above_all.state(), ProjectionRecallState::Current);
    assert!(above_all.searched);
    assert!(above_all.hits.is_empty());

    drop(client);
    fixture.server.stop().await;
}

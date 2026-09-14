use ackplane_client::companion::wire::{read_message, write_message, NodeReply, MAX_MESSAGE_BYTES};
use ackplane_protocol::v1::{ProjectionRecallHit, RecallProjectedNodesResult};

use crate::projection::StructuralFact;

use super::*;

#[tokio::test]
async fn large_recall_pages_fit_companion_frames_and_an_oversized_first_hit_is_refused() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let model = "m".repeat(128);
    let query = unit_vector(1.0);
    let sources: Vec<_> = (0..200)
        .map(|index| ProjectionEmbeddingSource {
            node_id: format!("{index:0>2048}"),
            label: "x".repeat(4096),
        })
        .collect();
    fixture.project(&sources).await;
    let mut client = fixture.server.client().await;
    for (index, source) in sources.iter().enumerate() {
        let similarity = if index < 50 {
            0.94 - index as f32 * 0.001
        } else {
            0.10
        };
        assert!(
            client
                .publish_projection_embedding(fixture.publish(
                    source,
                    &model,
                    &unit_vector(similarity)
                ))
                .await
                .unwrap()
                .into_inner()
                .stored
        );
    }

    for (limit, expected_count) in [(0, 10), (1, 1), (12, 12)] {
        let limited = client
            .recall_projected_nodes(fixture.recall_request(&model, &query, 0.5, limit))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(limited.state(), ProjectionRecallState::Current);
        assert!(limited.searched);
        assert_eq!(limited.hits.len(), expected_count);
        assert!(!limited.truncated);
        assert_eq!(limited.hits[0].node_id, sources[0].node_id);
    }

    let response = client
        .recall_projected_nodes(fixture.recall_request(&model, &query, 0.5, 100))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(response.state(), ProjectionRecallState::Current);
    assert_eq!(response.projected_nodes, 200);
    assert_eq!(response.embedded_nodes, 200);
    assert_eq!(response.projected_position, Some(response.ledger_position));
    assert!(response.projected_at.is_some());
    assert!(response.searched);
    assert_eq!(response.model, model);
    assert!(response.truncated);
    assert!(!response.hits.is_empty());
    assert!(response.hits.len() < 50);
    assert!(response.encoded_len() <= 192 * 1024);
    for (index, hit) in response.hits.iter().enumerate() {
        assert_eq!(hit.node_id, sources[index].node_id);
        assert_eq!(hit.label, sources[index].label);
        assert_eq!(hit.node_type, "artifact");
        assert!((hit.similarity - (0.94 - index as f32 * 0.001)).abs() < 1e-6);
    }
    let next_index = response.hits.len();
    let mut one_more = response.clone();
    one_more.hits.push(ProjectionRecallHit {
        node_id: sources[next_index].node_id.clone(),
        label: sources[next_index].label.clone(),
        node_type: "artifact".into(),
        similarity: 0.94 - next_index as f32 * 0.001,
    });
    assert!(one_more.encoded_len() > 192 * 1024);

    let mut frame = Vec::new();
    write_message(&mut frame, &NodeReply::Payload(response.encode_to_vec()))
        .await
        .expect("the actual JSON-wrapped protobuf reply must fit the companion frame");
    assert!(frame.len() <= MAX_MESSAGE_BYTES + 4);
    let decoded: NodeReply = read_message(&mut frame.as_slice()).await.unwrap();
    let NodeReply::Payload(payload) = decoded else {
        panic!("the companion frame lost the recall payload");
    };
    assert_eq!(
        RecallProjectedNodesResult::decode(payload.as_slice()).unwrap(),
        response
    );
    eprintln!(
        "recall bound: 200 indexed sources, 50 high scores, {} whole hits, {} protobuf bytes, {} framed bytes",
        response.hits.len(), response.encoded_len(), frame.len()
    );
    drop(client);
    fixture.server.stop().await;

    let oversized = Fixture::new().await;
    let source = ProjectionEmbeddingSource {
        node_id: "artifact:private-oversized-kind".into(),
        label: "private oversized source".into(),
    };
    oversized
        .append_facts_in(
            &oversized.binding.tenant_id,
            &oversized.binding.repository_id,
            &[StructuralFact {
                node_id: source.node_id.clone(),
                node_type: format!("private-kind-{}", "x".repeat(192 * 1024)),
                label: source.label.clone(),
                edges: vec![],
            }],
        )
        .await;
    oversized
        .projector
        .rebuild(
            &oversized.binding.tenant_id,
            &oversized.binding.repository_id,
        )
        .await
        .unwrap();
    let mut client = oversized.server.client().await;
    assert!(
        client
            .publish_projection_embedding(oversized.publish(&source, &model, &query))
            .await
            .unwrap()
            .into_inner()
            .stored
    );
    let probe = client
        .recall_projected_nodes(oversized.recall_request(&model, &[], 0.0, 10))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(probe.state(), ProjectionRecallState::Current);
    assert_eq!(probe.embedded_nodes, 1);
    assert!(!probe.searched);
    assert!(probe.hits.is_empty());
    let error = client
        .recall_projected_nodes(oversized.recall_request(&model, &query, 0.0, 10))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::FailedPrecondition);
    assert_eq!(error.message(), "a recall hit exceeds the response limit");
    assert!(!error.message().contains("private"));
    drop(client);
    oversized.server.stop().await;
}

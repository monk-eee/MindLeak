use ackplane_client::companion::wire::{write_message, NodeReply};
use ackplane_protocol::v1::ProjectionEmbeddingSource;
use prost::Message;

use super::support::Fixture;
use crate::projection::tests::require_test_database;

// Regression: a valid 100-row protobuf page exceeded the companion's JSON
// frame limit, making indexing fail for large labels. Bound whole sources by bytes.
#[tokio::test]
async fn large_source_pages_fit_the_companion_frame_without_truncating_labels() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let sources: Vec<_> = (0..100)
        .map(|index| ProjectionEmbeddingSource {
            node_id: format!("{:0>2048}", index),
            label: "x".repeat(4096),
        })
        .collect();
    fixture.project(&sources).await;
    let mut client = fixture.server.client().await;
    let response = client
        .list_missing_projection_embeddings(fixture.list("fixture-model", 100))
        .await
        .expect("valid sources must produce a usable page")
        .into_inner();
    let mut frame = Vec::new();
    write_message(&mut frame, &NodeReply::Payload(response.encode_to_vec()))
        .await
        .expect("the page must fit the actual companion frame");
    assert!(!response.nodes.is_empty());
    assert!(response.nodes.len() < sources.len());
    assert!(response.has_more);
    assert!(response.nodes.iter().all(|node| sources.contains(node)));
    drop(client);
    fixture.server.stop().await;
}

#[tokio::test]
async fn sources_outside_the_embedding_input_contract_are_not_offered_as_indexable() {
    let _database_url = require_test_database!();
    for source in [
        ProjectionEmbeddingSource {
            node_id: "artifact:oversized-label.rs".into(),
            label: "private-label".repeat(400),
        },
        ProjectionEmbeddingSource {
            node_id: "n".repeat(2049),
            label: "label".into(),
        },
    ] {
        let fixture = Fixture::new().await;
        fixture.project(&[source]).await;
        let mut client = fixture.server.client().await;
        let error = client
            .list_missing_projection_embeddings(fixture.list("fixture-model", 20))
            .await
            .expect_err("a source the writer cannot accept must not be offered as indexable");
        assert_eq!(error.code(), tonic::Code::FailedPrecondition);
        assert!(!error.message().contains("private-label"));
        drop(client);
        fixture.server.stop().await;
    }
}

#[tokio::test]
async fn oversized_requests_are_refused_by_the_grpc_transport_limit() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let source = ProjectionEmbeddingSource {
        node_id: "artifact:oversized-request.rs".into(),
        label: "bounded request".into(),
    };
    fixture.project(std::slice::from_ref(&source)).await;
    let mut client = fixture.server.client().await;
    let request = fixture.publish(&source, "fixture-model", &vec![0.25; 20_000]);
    let error = client
        .publish_projection_embedding(request)
        .await
        .unwrap_err();
    assert_eq!(error.code(), tonic::Code::OutOfRange);
    assert!(fixture.embeddings().await.is_empty());
    drop(client);
    fixture.server.stop().await;
}

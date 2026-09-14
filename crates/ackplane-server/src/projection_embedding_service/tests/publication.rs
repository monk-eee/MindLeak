use super::*;

#[tokio::test]
async fn companion_publishes_the_requested_vector_and_preserves_model_independence() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let source = ProjectionEmbeddingSource {
        node_id: "artifact:fixture.rs".into(),
        label: "projection embedding fixture".into(),
    };
    fixture.project(std::slice::from_ref(&source)).await;
    let directory = tempfile::Builder::new().prefix("pe-").tempdir().unwrap();
    let companion = TestCompanion::start(
        &fixture.server.endpoint,
        fixture.binding.clone(),
        &[11; 32],
        directory.path(),
    )
    .await;
    let mut client = NodeClient::new(
        directory.path().into(),
        fixture.binding.tenant_id.clone(),
        fixture.binding.repository_id.clone(),
    );
    client.expected_endpoint = Some(fixture.server.endpoint.clone());
    let pending = client
        .protobuf::<ListMissingProjectionEmbeddingsResult>(Operation::ProjectionEmbeddingsMissing {
            model: "fixture-model".into(),
            limit: 0,
        })
        .await
        .unwrap();
    assert_eq!(pending.nodes, vec![source.clone()]);
    assert!(!pending.has_more);

    let mut embedding = vec![0.0; 768];
    embedding[7] = 0.6;
    embedding[31] = 0.8;
    let published = client
        .protobuf::<PublishProjectionEmbeddingResult>(Operation::ProjectionEmbeddingPublish {
            source: pending.nodes[0].encode_to_vec(),
            model: "fixture-model".into(),
            embedding: embedding.clone(),
        })
        .await
        .unwrap();
    assert!(published.stored);
    let missing = client
        .protobuf::<ListMissingProjectionEmbeddingsResult>(Operation::ProjectionEmbeddingsMissing {
            model: "fixture-model".into(),
            limit: 20,
        })
        .await
        .unwrap();
    assert!(missing.nodes.is_empty());
    assert!(!missing.has_more);
    let other_model = client
        .protobuf::<ListMissingProjectionEmbeddingsResult>(Operation::ProjectionEmbeddingsMissing {
            model: "another-model".into(),
            limit: 20,
        })
        .await
        .unwrap();
    assert_eq!(other_model.nodes, vec![source.clone()]);
    assert!(!other_model.has_more);
    assert_eq!(
        fixture.embeddings().await,
        vec![(
            source.node_id.clone(),
            "fixture-model".into(),
            embedding.clone()
        )]
    );
    let ranked = fixture
        .projector
        .similar_nodes(
            &fixture.binding.tenant_id,
            &fixture.binding.repository_id,
            "fixture-model",
            &embedding,
            10,
        )
        .await
        .unwrap();
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].node_id, source.node_id);
    assert_eq!(ranked[0].label, source.label);
    assert!(ranked[0].cosine_distance.abs() < 1e-6, "{ranked:?}");

    let mut mismatched = client.clone();
    mismatched.tenant_id = unique_id("wrong-tenant");
    let error = mismatched
        .request(Operation::ProjectionEmbeddingsMissing {
            model: "fixture-model".into(),
            limit: 20,
        })
        .await
        .unwrap_err();
    assert!(
        matches!(
            error,
            ClientError::ConnectionRefused {
                retryable: false,
                ..
            }
        ),
        "{error:?}"
    );
    drop(companion);
    fixture.server.stop().await;
}

#[tokio::test]
async fn invalid_inputs_return_invalid_argument_without_writing_embeddings() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let source = ProjectionEmbeddingSource {
        node_id: "artifact:validated.rs".into(),
        label: "validated source".into(),
    };
    fixture.project(std::slice::from_ref(&source)).await;
    let mut client = fixture.server.client().await;
    for case in [
        "empty",
        "short",
        "long",
        "nan",
        "infinity",
        "negative_infinity",
        "zero",
    ] {
        let mut vector = vec![0.25; 768];
        match case {
            "empty" => vector.clear(),
            "short" => {
                vector.pop();
            }
            "long" => vector.push(0.25),
            "nan" => vector[3] = f32::NAN,
            "infinity" => vector[3] = f32::INFINITY,
            "negative_infinity" => vector[3] = f32::NEG_INFINITY,
            "zero" => vector.fill(0.0),
            _ => unreachable!(),
        }
        let error = client
            .publish_projection_embedding(fixture.publish(&source, "fixture-model", &vector))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::InvalidArgument, "{case}: {error}");
        assert!(
            fixture.embeddings().await.is_empty(),
            "{case} wrote a vector"
        );
    }
    for model in [String::new(), " \t".into(), "m".repeat(129)] {
        let error = client
            .list_missing_projection_embeddings(fixture.list(&model, 20))
            .await
            .unwrap_err();
        assert_eq!(
            error.code(),
            Code::InvalidArgument,
            "list model {model:?}: {error}"
        );
        let error = client
            .publish_projection_embedding(fixture.publish(&source, &model, &vec![0.25; 768]))
            .await
            .unwrap_err();
        assert_eq!(
            error.code(),
            Code::InvalidArgument,
            "publish model {model:?}: {error}"
        );
        assert!(fixture.embeddings().await.is_empty());
    }
    let error = client
        .list_missing_projection_embeddings(fixture.list("fixture-model", 101))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::InvalidArgument);
    let mut no_source = fixture.publish(&source, "fixture-model", &vec![0.25; 768]);
    no_source.source = None;
    let error = client
        .publish_projection_embedding(no_source)
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::InvalidArgument);
    assert_eq!(error.message(), "source is required");

    for case in [
        "empty_node_id",
        "blank_node_id",
        "oversized_node_id",
        "oversized_label",
    ] {
        let mut invalid = source.clone();
        match case {
            "empty_node_id" => invalid.node_id.clear(),
            "blank_node_id" => invalid.node_id = " \t".into(),
            "oversized_node_id" => invalid.node_id = "n".repeat(2049),
            "oversized_label" => invalid.label = "l".repeat(4097),
            _ => unreachable!(),
        }
        let error = client
            .publish_projection_embedding(fixture.publish(
                &invalid,
                "fixture-model",
                &vec![0.25; 768],
            ))
            .await
            .unwrap_err();
        assert_eq!(error.code(), Code::InvalidArgument, "{case}: {error}");
    }
    assert!(fixture.embeddings().await.is_empty());
    let pending = client
        .list_missing_projection_embeddings(fixture.list("fixture-model", 20))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(pending.nodes, vec![source]);
    assert!(!pending.has_more);
    drop(client);
    fixture.server.stop().await;
}

#[tokio::test]
async fn stale_or_absent_sources_cannot_replace_a_current_vector_or_create_nodes() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let old_source = ProjectionEmbeddingSource {
        node_id: "artifact:updated.rs".into(),
        label: "old label".into(),
    };
    fixture.project(std::slice::from_ref(&old_source)).await;
    let mut client = fixture.server.client().await;
    let pending = client
        .list_missing_projection_embeddings(fixture.list("fixture-model", 20))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(pending.nodes, vec![old_source.clone()]);
    let stale_request = fixture.publish(&pending.nodes[0], "fixture-model", &vec![0.9; 768]);
    let current = ProjectionEmbeddingSource {
        node_id: old_source.node_id.clone(),
        label: "current label".into(),
    };
    fixture.project(std::slice::from_ref(&current)).await;
    let vector = vec![0.25; 768];
    let stored = client
        .publish_projection_embedding(fixture.publish(&current, "fixture-model", &vector))
        .await
        .unwrap()
        .into_inner();
    assert!(stored.stored);
    let rejected = client
        .publish_projection_embedding(stale_request)
        .await
        .unwrap()
        .into_inner();
    assert!(
        !rejected.stored,
        "a delayed upload for the old label overwrote the new vector"
    );
    let expected = vec![(current.node_id.clone(), "fixture-model".into(), vector)];
    assert_eq!(fixture.embeddings().await, expected);

    for case in ["wrong_label", "missing_label", "unprojected_id"] {
        let mut invalid = current.clone();
        match case {
            "wrong_label" => invalid.label = "unrelated label".into(),
            "missing_label" => invalid.label.clear(),
            "unprojected_id" => invalid.node_id = "artifact:unprojected.rs".into(),
            _ => unreachable!(),
        }
        let rejected = client
            .publish_projection_embedding(fixture.publish(
                &invalid,
                "fixture-model",
                &vec![0.9; 768],
            ))
            .await
            .unwrap()
            .into_inner();
        assert!(
            !rejected.stored,
            "{case} must be a conditional-write refusal"
        );
        assert_eq!(
            fixture.embeddings().await,
            expected,
            "{case} changed the current vector"
        );
    }
    let rows = fixture.pool.get().await.unwrap().query(
        "SELECT node_id, label FROM projected_nodes WHERE tenant_id = $1 AND repository_id = $2",
        &[&fixture.binding.tenant_id, &fixture.binding.repository_id],
    ).await.unwrap();
    assert_eq!(
        rows.len(),
        1,
        "an upload created a node outside the ledger projection"
    );
    assert_eq!(rows[0].get::<_, String>(0), current.node_id);
    assert_eq!(rows[0].get::<_, String>(1), current.label);
    drop(client);
    fixture.server.stop().await;
}

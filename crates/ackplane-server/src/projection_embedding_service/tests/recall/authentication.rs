use super::*;

#[tokio::test]
async fn recall_authentication_binds_all_inputs_without_consuming_rejected_nonces() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let mut client = fixture.server.client().await;
    let model = "recall-model";
    let query = unit_vector(1.0);
    let source = ProjectionEmbeddingSource {
        node_id: "artifact:authenticated-recall.rs".into(),
        label: "authenticated recall source".into(),
    };
    fixture.project(std::slice::from_ref(&source)).await;
    assert!(
        client
            .publish_projection_embedding(fixture.publish(&source, model, &query))
            .await
            .unwrap()
            .into_inner()
            .stored
    );

    let mut unsigned = fixture.recall_request(model, &query, 0.5, 10);
    unsigned.authentication = None;
    assert_eq!(
        client
            .recall_projected_nodes(unsigned)
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );

    for field in [
        "tenant",
        "repository",
        "model",
        "query_component",
        "query_sign_bit",
        "probe",
        "floor",
        "limit",
        "default_limit",
        "signature",
        "short_query",
        "long_query",
        "zero_query",
        "nan_query",
        "infinite_query",
        "negative_floor",
        "large_floor",
        "nan_floor",
        "infinite_floor",
        "large_limit",
        "empty_model",
        "large_model",
    ] {
        let authentic = fixture.recall_request(model, &query, 0.5, 10);
        let mut changed = authentic.clone();
        let expected = match field {
            "tenant" => {
                changed.tenant_id = unique_id("other-tenant");
                Code::PermissionDenied
            }
            "repository" => {
                changed.repository_id = unique_id("other-repository");
                Code::PermissionDenied
            }
            "model" => {
                changed.model = "forged-model".into();
                Code::Unauthenticated
            }
            "query_component" => {
                changed.query_embedding[0] = 0.75;
                Code::Unauthenticated
            }
            "query_sign_bit" => {
                changed.query_embedding[2] = -0.0;
                Code::Unauthenticated
            }
            "probe" => {
                changed.query_embedding.clear();
                Code::Unauthenticated
            }
            "floor" => {
                changed.floor = 0.6;
                Code::Unauthenticated
            }
            "limit" => {
                changed.limit = 11;
                Code::Unauthenticated
            }
            "default_limit" => {
                changed.limit = 0;
                Code::Unauthenticated
            }
            "signature" => {
                changed.authentication.as_mut().unwrap().signature[0] ^= 1;
                Code::Unauthenticated
            }
            "short_query" => {
                changed.query_embedding.pop();
                Code::InvalidArgument
            }
            "long_query" => {
                changed.query_embedding.push(0.0);
                Code::InvalidArgument
            }
            "zero_query" => {
                changed.query_embedding.fill(0.0);
                Code::InvalidArgument
            }
            "nan_query" => {
                changed.query_embedding[1] = f32::NAN;
                Code::InvalidArgument
            }
            "infinite_query" => {
                changed.query_embedding[1] = f32::INFINITY;
                Code::InvalidArgument
            }
            "negative_floor" => {
                changed.floor = -0.1;
                Code::InvalidArgument
            }
            "large_floor" => {
                changed.floor = 1.1;
                Code::InvalidArgument
            }
            "nan_floor" => {
                changed.floor = f32::NAN;
                Code::InvalidArgument
            }
            "infinite_floor" => {
                changed.floor = f32::INFINITY;
                Code::InvalidArgument
            }
            "large_limit" => {
                changed.limit = 101;
                Code::InvalidArgument
            }
            "empty_model" => {
                changed.model = " ".into();
                Code::InvalidArgument
            }
            "large_model" => {
                changed.model = "m".repeat(129);
                Code::InvalidArgument
            }
            _ => unreachable!(),
        };
        let error = client.recall_projected_nodes(changed).await.unwrap_err();
        assert_eq!(error.code(), expected, "changed {field}: {error}");
        let accepted = client
            .recall_projected_nodes(authentic)
            .await
            .unwrap_or_else(|error| panic!("changed {field} burned the authentic nonce: {error}"))
            .into_inner();
        assert_eq!(accepted.state(), ProjectionRecallState::Current);
        assert!(accepted.searched);
        assert_eq!(accepted.hits.len(), 1);
        assert_eq!(accepted.hits[0].node_id, source.node_id);
    }

    let list = fixture.list(model, 10);
    let mut copied_list = fixture.recall_request(model, &query, 0.5, 10);
    copied_list.authentication = list.authentication.clone();
    assert_eq!(
        client
            .recall_projected_nodes(copied_list)
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
    assert!(client
        .list_missing_projection_embeddings(list)
        .await
        .expect("a copied list signature must not burn its nonce")
        .into_inner()
        .nodes
        .is_empty());

    let publish = fixture.publish(&source, model, &query);
    let mut copied_publish = fixture.recall_request(model, &query, 0.5, 10);
    copied_publish.authentication = publish.authentication.clone();
    assert_eq!(
        client
            .recall_projected_nodes(copied_publish)
            .await
            .unwrap_err()
            .code(),
        Code::Unauthenticated
    );
    assert!(
        client
            .publish_projection_embedding(publish)
            .await
            .expect("a copied publish signature must not burn its nonce")
            .into_inner()
            .stored
    );

    for destination in ["list", "publish"] {
        let authentic = fixture.recall_request(model, &query, 0.5, 10);
        let error = match destination {
            "list" => {
                let mut copied = fixture.list(model, 10);
                copied.authentication = authentic.authentication.clone();
                client
                    .list_missing_projection_embeddings(copied)
                    .await
                    .unwrap_err()
            }
            "publish" => {
                let mut copied = fixture.publish(&source, model, &query);
                copied.authentication = authentic.authentication.clone();
                client
                    .publish_projection_embedding(copied)
                    .await
                    .unwrap_err()
            }
            _ => unreachable!(),
        };
        assert_eq!(
            error.code(),
            Code::Unauthenticated,
            "copied to {destination}"
        );
        client
            .recall_projected_nodes(authentic)
            .await
            .unwrap_or_else(|error| panic!("copying to {destination} burned the nonce: {error}"));
    }

    let replay = fixture.recall_request(model, &query, 0.5, 10);
    client.recall_projected_nodes(replay.clone()).await.unwrap();
    let error = client.recall_projected_nodes(replay).await.unwrap_err();
    assert_eq!(error.code(), Code::Unauthenticated);
    assert_eq!(
        error.message(),
        "projection embedding authentication was already used"
    );

    let simultaneous = fixture.recall_request(model, &query, 0.5, 10);
    let mut other_client = fixture.server.client().await;
    let (first, second) = tokio::join!(
        client.recall_projected_nodes(simultaneous.clone()),
        other_client.recall_projected_nodes(simultaneous),
    );
    let outcomes = [first, second];
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_ok()).count(),
        1,
        "{outcomes:?}"
    );
    for outcome in outcomes {
        match outcome {
            Ok(response) => {
                let response = response.into_inner();
                assert_eq!(response.state(), ProjectionRecallState::Current);
                assert_eq!(response.hits.len(), 1);
                assert_eq!(response.hits[0].node_id, source.node_id);
            }
            Err(error) => {
                assert_eq!(error.code(), Code::Unauthenticated);
                assert_eq!(
                    error.message(),
                    "projection embedding authentication was already used"
                );
            }
        }
    }
    assert_eq!(
        fixture.embeddings().await,
        vec![(source.node_id, model.into(), query)]
    );

    drop(client);
    drop(other_client);
    fixture.server.stop().await;
}

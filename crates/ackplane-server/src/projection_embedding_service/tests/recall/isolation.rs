use super::*;

#[tokio::test]
async fn matching_node_ids_stay_scoped_and_revocation_refuses_pending_and_fresh_recall() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let mut client = fixture.server.client().await;
    let model = "recall-model";
    let query = unit_vector(1.0);
    let source = ProjectionEmbeddingSource {
        node_id: "artifact:shared-name.rs".into(),
        label: "local source".into(),
    };
    fixture.project(std::slice::from_ref(&source)).await;
    assert!(
        client
            .publish_projection_embedding(fixture.publish(&source, model, &unit_vector(0.0)))
            .await
            .unwrap()
            .into_inner()
            .stored
    );
    let before = client
        .recall_projected_nodes(fixture.recall_request(model, &query, 0.5, 10))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(before.state(), ProjectionRecallState::Current);
    assert_eq!(before.projected_nodes, 1);
    assert_eq!(before.embedded_nodes, 1);
    assert!(before.searched);
    assert!(before.hits.is_empty());

    for (tenant_id, repository_id) in [
        (
            fixture.binding.tenant_id.clone(),
            unique_id("foreign-repository"),
        ),
        (
            unique_id("foreign-tenant"),
            fixture.binding.repository_id.clone(),
        ),
    ] {
        let foreign = Fixture::new_in(tenant_id, repository_id).await;
        let sources = [
            ProjectionEmbeddingSource {
                node_id: source.node_id.clone(),
                label: "private foreign collision".into(),
            },
            ProjectionEmbeddingSource {
                node_id: "artifact:foreign-only.rs".into(),
                label: "private foreign-only source".into(),
            },
        ];
        foreign.project(&sources).await;
        let mut foreign_client = foreign.server.client().await;
        for foreign_source in &sources {
            assert!(
                foreign_client
                    .publish_projection_embedding(foreign.publish(foreign_source, model, &query))
                    .await
                    .unwrap()
                    .into_inner()
                    .stored
            );
        }
        let foreign_result = foreign_client
            .recall_projected_nodes(foreign.recall_request(model, &query, 0.5, 10))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(foreign_result.state(), ProjectionRecallState::Current);
        assert_eq!(foreign_result.projected_nodes, 2);
        assert_eq!(foreign_result.embedded_nodes, 2);
        assert_eq!(foreign_result.hits.len(), 2);
        assert!(foreign_result.hits.iter().all(|hit| sources
            .iter()
            .any(|source| source.node_id == hit.node_id && source.label == hit.label)));
        drop(foreign_client);
        foreign.server.stop().await;

        let after = client
            .recall_projected_nodes(fixture.recall_request(model, &query, 0.5, 10))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(
            after, before,
            "foreign data changed local recall or its metadata"
        );
    }

    assert!(
        client
            .publish_projection_embedding(fixture.publish(&source, model, &query))
            .await
            .unwrap()
            .into_inner()
            .stored
    );
    let local = client
        .recall_projected_nodes(fixture.recall_request(model, &query, 0.5, 10))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(local.state(), ProjectionRecallState::Current);
    assert_eq!(local.projected_nodes, 1);
    assert_eq!(local.embedded_nodes, 1);
    assert_eq!(local.hits.len(), 1);
    assert_eq!(local.hits[0].node_id, source.node_id);
    assert_eq!(local.hits[0].label, source.label);
    assert_eq!(local.hits[0].node_type, "artifact");
    assert_eq!(local.hits[0].similarity, 1.0);

    let pending = fixture.recall_request(model, &query, 0.5, 10);
    let mut connection = fixture.pool.get().await.unwrap();
    let transaction = connection.transaction().await.unwrap();
    assert!(signing_keys::revoke(
        &transaction,
        &KeyRevocation {
            signing_key_id: fixture.binding.key_id.clone(),
            reason: "recall integration fixture".into(),
        },
        std::time::SystemTime::now(),
    )
    .await
    .unwrap());
    transaction.commit().await.unwrap();
    drop(connection);
    for request in [pending, fixture.recall_request(model, &[], 0.5, 10)] {
        let error = client.recall_projected_nodes(request).await.unwrap_err();
        assert_eq!(error.code(), Code::Unauthenticated);
        assert_eq!(error.message(), "signing key has been revoked");
    }
    assert_eq!(
        fixture.embeddings().await,
        vec![(source.node_id, model.into(), query)]
    );

    drop(client);
    fixture.server.stop().await;
}

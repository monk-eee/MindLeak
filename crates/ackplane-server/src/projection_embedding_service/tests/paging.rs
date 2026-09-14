use super::*;

#[tokio::test]
async fn missing_list_is_bounded_and_cannot_leak_same_id_sources_from_other_scopes() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let other_tenant = unique_id("other-tenant");
    let other_repository = unique_id("other-repository");
    for (tenant, repository, label) in [
        (
            &other_tenant,
            &fixture.binding.repository_id,
            "other tenant label",
        ),
        (
            &fixture.binding.tenant_id,
            &other_repository,
            "other repository label",
        ),
    ] {
        fixture
            .project_in(
                tenant,
                repository,
                &[
                    ProjectionEmbeddingSource {
                        node_id: "artifact:shared.rs".into(),
                        label: label.into(),
                    },
                    ProjectionEmbeddingSource {
                        node_id: "artifact:foreign-only.rs".into(),
                        label: label.into(),
                    },
                ],
            )
            .await;
    }
    let mut client = fixture.server.client().await;
    let empty = client
        .list_missing_projection_embeddings(fixture.list("fixture-model", 0))
        .await
        .unwrap()
        .into_inner();
    assert!(
        empty.nodes.is_empty(),
        "unprojected scope leaked a foreign source: {empty:?}"
    );
    assert!(!empty.has_more);

    let mut sources: Vec<_> = (0..20)
        .map(|index| ProjectionEmbeddingSource {
            node_id: format!("artifact:own-{index}.rs"),
            label: format!("own source {index}"),
        })
        .collect();
    sources.push(ProjectionEmbeddingSource {
        node_id: "artifact:shared.rs".into(),
        label: "own shared label".into(),
    });
    fixture.project(&sources).await;
    for (limit, expected_count, expected_more) in [(0, 20, true), (1, 1, true), (100, 21, false)] {
        let result = client
            .list_missing_projection_embeddings(fixture.list("fixture-model", limit))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(result.nodes.len(), expected_count, "limit {limit}");
        assert_eq!(result.has_more, expected_more, "limit {limit}");
        assert!(
            result.nodes.iter().all(|node| sources.contains(node)),
            "limit {limit} leaked {result:?}"
        );
        let ids: std::collections::HashSet<_> =
            result.nodes.iter().map(|node| &node.node_id).collect();
        assert_eq!(
            ids.len(),
            result.nodes.len(),
            "limit {limit} duplicated a node"
        );
    }
    let mut all = client
        .list_missing_projection_embeddings(fixture.list("fixture-model", 100))
        .await
        .unwrap()
        .into_inner();
    all.nodes
        .sort_by(|left, right| left.node_id.cmp(&right.node_id));
    sources.sort_by(|left, right| left.node_id.cmp(&right.node_id));
    assert_eq!(all.nodes, sources);
    assert!(!all.has_more);
    drop(client);
    fixture.server.stop().await;
}

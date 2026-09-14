use crate::projection::StructuralFact;

use super::*;

#[tokio::test]
async fn probes_and_searches_report_real_freshness_and_model_specific_coverage() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let mut client = fixture.server.client().await;
    let model = "recall-model";
    let query = unit_vector(1.0);
    let sources = [
        ProjectionEmbeddingSource {
            node_id: "artifact:first.rs".into(),
            label: "first recall source".into(),
        },
        ProjectionEmbeddingSource {
            node_id: "artifact:second.rs".into(),
            label: "second recall source".into(),
        },
    ];
    let facts: Vec<_> = sources
        .iter()
        .map(|source| StructuralFact {
            node_id: source.node_id.clone(),
            node_type: "artifact".into(),
            label: source.label.clone(),
            edges: vec![],
        })
        .collect();
    fixture
        .append_facts_in(
            &fixture.binding.tenant_id,
            &fixture.binding.repository_id,
            &facts,
        )
        .await;

    let unprojected = client
        .recall_projected_nodes(fixture.recall_request(model, &[], 0.5, 10))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(unprojected.state(), ProjectionRecallState::NotYetProjected);
    assert_eq!(unprojected.projected_nodes, 0);
    assert_eq!(unprojected.embedded_nodes, 0);
    assert!(unprojected.ledger_position > 0);
    assert_eq!(unprojected.projected_position, None);
    assert_eq!(unprojected.projected_at, None);
    assert!(!unprojected.searched);
    assert!(!unprojected.truncated);
    assert!(unprojected.hits.is_empty());
    let unprojected_search = client
        .recall_projected_nodes(fixture.recall_request(model, &query, 0.5, 10))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(unprojected_search, unprojected);

    let summary = fixture
        .projector
        .rebuild(&fixture.binding.tenant_id, &fixture.binding.repository_id)
        .await
        .unwrap();
    assert_eq!(summary.nodes, 2);
    assert_eq!(summary.stream_position, unprojected.ledger_position);

    for (embedded_count, expected_state) in [
        (0, ProjectionRecallState::NotYetEmbedded),
        (1, ProjectionRecallState::PartiallyEmbedded),
        (2, ProjectionRecallState::Current),
    ] {
        if embedded_count > 0 {
            let published = client
                .publish_projection_embedding(fixture.publish(
                    &sources[(embedded_count - 1) as usize],
                    model,
                    &query,
                ))
                .await
                .unwrap()
                .into_inner();
            assert!(published.stored);
        }
        let probe = client
            .recall_projected_nodes(fixture.recall_request(model, &[], 0.5, 10))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(probe.state(), expected_state);
        assert_eq!(probe.projected_nodes, 2);
        assert_eq!(probe.embedded_nodes, embedded_count);
        assert_eq!(probe.ledger_position, summary.stream_position);
        assert_eq!(probe.projected_position, Some(summary.stream_position));
        OffsetDateTime::parse(probe.projected_at.as_deref().unwrap(), &Rfc3339).unwrap();
        assert_eq!(probe.model, model);
        assert!(probe.hits.is_empty());
        assert!(!probe.searched);
        assert!(!probe.truncated);

        let search = client
            .recall_projected_nodes(fixture.recall_request(model, &query, 0.5, 10))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(search.state(), expected_state);
        assert_eq!(search.projected_nodes, probe.projected_nodes);
        assert_eq!(search.embedded_nodes, probe.embedded_nodes);
        assert_eq!(search.ledger_position, probe.ledger_position);
        assert_eq!(search.projected_position, probe.projected_position);
        assert_eq!(search.projected_at, probe.projected_at);
        assert_eq!(search.searched, embedded_count > 0);
        assert!(!search.truncated);
        assert_eq!(search.hits.len(), embedded_count as usize);
        for hit in search.hits {
            assert!(sources[..embedded_count as usize]
                .iter()
                .any(|source| source.node_id == hit.node_id && source.label == hit.label));
            assert_eq!(hit.node_type, "artifact");
            assert_eq!(hit.similarity, 1.0);
        }
    }

    let no_match = client
        .recall_projected_nodes(fixture.recall_request(model, &unit_vector(0.0), 0.5, 10))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(no_match.state(), ProjectionRecallState::Current);
    assert_eq!(no_match.projected_nodes, 2);
    assert_eq!(no_match.embedded_nodes, 2);
    assert!(no_match.searched);
    assert!(!no_match.truncated);
    assert!(no_match.hits.is_empty());

    let other_model = "other-recall-model";
    let unindexed = client
        .recall_projected_nodes(fixture.recall_request(other_model, &query, 0.5, 10))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(unindexed.state(), ProjectionRecallState::NotYetEmbedded);
    assert_eq!(unindexed.projected_nodes, 2);
    assert_eq!(unindexed.embedded_nodes, 0);
    assert_eq!(unindexed.projected_position, no_match.projected_position);
    assert_eq!(unindexed.model, other_model);
    assert!(!unindexed.searched);
    assert!(unindexed.hits.is_empty());
    assert!(
        client
            .publish_projection_embedding(fixture.publish(&sources[0], other_model, &query))
            .await
            .unwrap()
            .into_inner()
            .stored
    );
    let other_coverage = client
        .recall_projected_nodes(fixture.recall_request(other_model, &[], 0.5, 10))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        other_coverage.state(),
        ProjectionRecallState::PartiallyEmbedded
    );
    assert_eq!(other_coverage.embedded_nodes, 1);
    assert!(!other_coverage.searched);
    assert!(other_coverage.hits.is_empty());

    fixture
        .append_facts_in(
            &fixture.binding.tenant_id,
            &fixture.binding.repository_id,
            &[StructuralFact {
                node_id: sources[0].node_id.clone(),
                node_type: "artifact".into(),
                label: "changed after embedding".into(),
                edges: vec![],
            }],
        )
        .await;
    for stale_query in [&[][..], query.as_slice()] {
        let stale = client
            .recall_projected_nodes(fixture.recall_request(model, stale_query, 0.5, 10))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(stale.state(), ProjectionRecallState::Stale);
        assert_eq!(stale.projected_nodes, 2);
        assert_eq!(stale.embedded_nodes, 2);
        assert!(stale.ledger_position > summary.stream_position);
        assert_eq!(stale.projected_position, Some(summary.stream_position));
        assert_eq!(stale.projected_at, no_match.projected_at);
        assert!(!stale.searched);
        assert!(!stale.truncated);
        assert!(stale.hits.is_empty());
    }

    drop(client);
    fixture.server.stop().await;
}

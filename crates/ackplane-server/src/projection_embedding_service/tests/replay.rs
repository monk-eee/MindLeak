use super::*;

#[tokio::test]
async fn concurrent_publications_admit_one_nonce_and_restart_preserves_replay_refusal() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let source = ProjectionEmbeddingSource {
        node_id: "artifact:replayed.rs".into(),
        label: "replay fixture".into(),
    };
    fixture.project(std::slice::from_ref(&source)).await;
    let vector = vec![0.25; 768];
    let request = fixture.publish(&source, "fixture-model", &vector);
    let signed_at = OffsetDateTime::parse(
        &request.authentication.as_ref().unwrap().signed_at,
        &Rfc3339,
    )
    .unwrap();
    let mut first = fixture.server.client().await;
    let mut second = fixture.server.client().await;
    let (first_result, second_result) = tokio::join!(
        first.publish_projection_embedding(request.clone()),
        second.publish_projection_embedding(request.clone()),
    );
    let results = [first_result, second_result];
    assert_eq!(
        results.iter().filter(|result| result.is_ok()).count(),
        1,
        "{results:?}"
    );
    for result in results {
        match result {
            Ok(response) => assert!(response.into_inner().stored),
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
        vec![(source.node_id, "fixture-model".into(), vector)]
    );
    let fresh_request = fixture.list("fixture-model", 20);
    let pool = fixture.pool.clone();
    drop(first);
    drop(second);
    fixture.server.stop().await;

    let restarted = TestServer::start(&pool).await;
    let mut client = restarted.client().await;
    assert!((OffsetDateTime::now_utc() - signed_at).abs() < time::Duration::seconds(300));
    let error = client
        .publish_projection_embedding(request)
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::Unauthenticated);
    assert_eq!(
        error.message(),
        "projection embedding authentication was already used"
    );
    let accepted = client
        .list_missing_projection_embeddings(fresh_request)
        .await
        .unwrap()
        .into_inner();
    assert!(accepted.nodes.is_empty());
    assert!(!accepted.has_more);
    drop(client);
    restarted.stop().await;
}

use super::*;

#[tokio::test]
async fn unsigned_requests_are_refused_over_grpc() {
    let _database_url = require_test_database!();
    let pool = test_pool().expect("projection embedding RPC tests require PostgreSQL");
    let server = TestServer::start(&pool).await;
    let mut client = server.client().await;
    let error = client
        .list_missing_projection_embeddings(ListMissingProjectionEmbeddingsRequest {
            tenant_id: unique_id("embedding-tenant"),
            repository_id: unique_id("embedding-repository"),
            model: "fixture-model".into(),
            limit: 20,
            authentication: None,
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::Unauthenticated);
    let error = client
        .publish_projection_embedding(PublishProjectionEmbeddingRequest {
            tenant_id: unique_id("embedding-tenant"),
            repository_id: unique_id("embedding-repository"),
            source: Some(ProjectionEmbeddingSource {
                node_id: "artifact:fixture.rs".into(),
                label: "fixture source".into(),
            }),
            model: "fixture-model".into(),
            embedding: vec![0.25; 768],
            authentication: None,
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::Unauthenticated);
    drop(client);
    server.stop().await;
}

#[tokio::test]
async fn scope_and_payload_tampering_cannot_consume_authentic_nonces() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let source = ProjectionEmbeddingSource {
        node_id: "artifact:authenticated.rs".into(),
        label: "authenticated source".into(),
    };
    fixture.project(std::slice::from_ref(&source)).await;
    let mut client = fixture.server.client().await;
    for field in ["tenant", "repository", "model", "limit", "signature"] {
        let authentic = fixture.list("fixture-model", 20);
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
            "limit" => {
                changed.limit = 21;
                Code::Unauthenticated
            }
            "signature" => {
                changed.authentication.as_mut().unwrap().signature[0] ^= 1;
                Code::Unauthenticated
            }
            _ => unreachable!(),
        };
        let error = client
            .list_missing_projection_embeddings(changed)
            .await
            .unwrap_err();
        assert_eq!(error.code(), expected, "changed list {field}: {error}");
        let accepted = client
            .list_missing_projection_embeddings(authentic)
            .await
            .unwrap_or_else(|error| panic!("changed {field} burned the authentic nonce: {error}"))
            .into_inner();
        assert_eq!(accepted.nodes, vec![source.clone()]);
    }

    let mut vector = vec![0.0; 768];
    vector[0] = 1.0;
    let list = fixture.list("fixture-model", 20);
    let mut changed_operation = fixture.publish(&source, "fixture-model", &vector);
    changed_operation.authentication = list.authentication.clone();
    let error = client
        .publish_projection_embedding(changed_operation)
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::Unauthenticated);
    assert!(error.message().contains("signature does not verify"));
    client
        .list_missing_projection_embeddings(list)
        .await
        .unwrap();
    assert!(fixture.embeddings().await.is_empty());

    for field in ["source_id", "label", "model", "vector", "vector_sign_bit"] {
        let authentic = fixture.publish(&source, "fixture-model", &vector);
        let before = fixture.embeddings().await;
        let mut changed = authentic.clone();
        match field {
            "source_id" => changed.source.as_mut().unwrap().node_id.push_str(".forged"),
            "label" => changed.source.as_mut().unwrap().label.push_str(" forged"),
            "model" => changed.model = "forged-model".into(),
            "vector" => changed.embedding[0] = 2.0,
            "vector_sign_bit" => changed.embedding[1] = -0.0,
            _ => unreachable!(),
        }
        let error = client
            .publish_projection_embedding(changed)
            .await
            .unwrap_err();
        assert_eq!(
            error.code(),
            Code::Unauthenticated,
            "changed publish {field}: {error}"
        );
        assert!(
            error.message().contains("signature does not verify"),
            "{field}: {error}"
        );
        assert_eq!(
            fixture.embeddings().await,
            before,
            "changed publish {field} wrote data"
        );
        let accepted = client
            .publish_projection_embedding(authentic)
            .await
            .unwrap_or_else(|error| panic!("changed {field} burned the authentic nonce: {error}"))
            .into_inner();
        assert!(accepted.stored, "authentic publish after changed {field}");
    }
    assert_eq!(
        fixture.embeddings().await,
        vec![(source.node_id, "fixture-model".into(), vector)]
    );
    drop(client);
    fixture.server.stop().await;
}

#[tokio::test]
async fn knowledge_signature_is_not_embedding_authority_even_with_a_fresh_nonce() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let mut client = fixture.server.client().await;
    let authentic = fixture.list("fixture-model", 20);
    let authentication = authentic.authentication.as_ref().unwrap();
    let mut knowledge = KnowledgeAuthentication {
        signing_key_id: authentication.signing_key_id.clone(),
        node_id: authentication.node_id.clone(),
        signed_at: authentication.signed_at.clone(),
        nonce: authentication.nonce.clone(),
        signature: vec![],
    };
    knowledge.signature = SigningKey::from_bytes(&[11; 32])
        .sign(&knowledge_signing_bytes(
            &fixture.binding.tenant_id,
            &fixture.binding.repository_id,
            &KnowledgeOperation::History { limit: 20 },
            &knowledge,
        ))
        .to_bytes()
        .to_vec();
    let mut forged = authentic.clone();
    forged.authentication.as_mut().unwrap().signature = knowledge.signature;
    let error = client
        .list_missing_projection_embeddings(forged)
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::Unauthenticated);
    assert!(error.message().contains("signature does not verify"));
    let accepted = client
        .list_missing_projection_embeddings(authentic)
        .await
        .unwrap()
        .into_inner();
    assert!(accepted.nodes.is_empty());
    assert!(!accepted.has_more);
    assert!(fixture.embeddings().await.is_empty());
    drop(client);
    fixture.server.stop().await;
}

#[tokio::test]
async fn persisted_key_revocation_refuses_new_reads_and_previously_signed_writes() {
    let _database_url = require_test_database!();
    let fixture = Fixture::new().await;
    let source = ProjectionEmbeddingSource {
        node_id: "artifact:revocation.rs".into(),
        label: "revocation fixture".into(),
    };
    fixture.project(std::slice::from_ref(&source)).await;
    let mut client = fixture.server.client().await;
    let vector = vec![0.25; 768];
    let stored = client
        .publish_projection_embedding(fixture.publish(&source, "fixture-model", &vector))
        .await
        .unwrap()
        .into_inner();
    assert!(stored.stored);
    let pending_write = fixture.publish(&source, "fixture-model", &vec![0.9; 768]);
    let mut connection = fixture.pool.get().await.unwrap();
    let transaction = connection.transaction().await.unwrap();
    assert!(signing_keys::revoke(
        &transaction,
        &KeyRevocation {
            signing_key_id: fixture.binding.key_id.clone(),
            reason: "projection embedding integration fixture".into(),
        },
        std::time::SystemTime::now(),
    )
    .await
    .unwrap());
    transaction.commit().await.unwrap();
    drop(connection);
    let error = client
        .list_missing_projection_embeddings(fixture.list("fixture-model", 20))
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::Unauthenticated);
    assert_eq!(error.message(), "signing key has been revoked");
    let error = client
        .publish_projection_embedding(pending_write)
        .await
        .unwrap_err();
    assert_eq!(error.code(), Code::Unauthenticated);
    assert_eq!(error.message(), "signing key has been revoked");
    assert_eq!(
        fixture.embeddings().await,
        vec![(source.node_id, "fixture-model".into(), vector)]
    );
    drop(client);
    fixture.server.stop().await;
}

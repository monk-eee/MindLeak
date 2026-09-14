use super::{
    tests::{recover_request, request},
    *,
};

struct Fixture {
    store: ClaimStore,
    pool: PgPool,
    tenant: String,
    now: SystemTime,
}

impl Fixture {
    async fn new() -> Option<Self> {
        let pool = crate::test_support::test_pool()?;
        Some(Self {
            store: ClaimStore::connect(&pool).await.unwrap(),
            pool,
            tenant: format!("node-custody-{}", crate::test_support::uuid_ish()),
            now: SystemTime::UNIX_EPOCH + Duration::from_secs(1000),
        })
    }

    async fn snapshot(&self, task: &str) -> serde_json::Value {
        let json: String = self.pool.get().await.unwrap().query_one(
            "SELECT to_jsonb(claim)::text FROM delegated_claims claim WHERE tenant_id = $1 AND repository_id = 'repository' AND task_id = $2",
            &[&self.tenant, &task],
        ).await.unwrap().get(0);
        serde_json::from_str(&json).unwrap()
    }
}

// An owner string is not proof of node custody. Every owner mutation must reject
// a peer's copy of that string without changing lease, scope, or parked state.
#[tokio::test]
async fn peer_owner_mutations_leave_the_original_grant_unchanged() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    let original = request(&fixture.tenant, "task", "owner");
    fixture
        .store
        .delegate(&original, fixture.now)
        .await
        .unwrap();
    let before = fixture.snapshot("task").await;
    let peer = ClaimOwner {
        owner_id: "owner",
        node_id: "peer-node",
    };
    let later = fixture.now + Duration::from_secs(1);
    assert!(matches!(
        fixture
            .store
            .release(&fixture.tenant, "repository", "task", peer, later)
            .await,
        Err(ClaimStoreError::OwnerNodeMismatch)
    ));
    assert!(matches!(
        fixture
            .store
            .renew(
                &fixture.tenant,
                "repository",
                "task",
                peer,
                Duration::from_secs(600),
                later
            )
            .await,
        Err(ClaimStoreError::OwnerNodeMismatch)
    ));
    assert!(matches!(
        fixture
            .store
            .park(&fixture.tenant, "repository", "task", peer, later)
            .await,
        Err(ClaimStoreError::OwnerNodeMismatch)
    ));
    assert!(matches!(
        fixture
            .store
            .answer(
                &fixture.tenant,
                "repository",
                "task",
                peer,
                Duration::from_secs(600),
                later
            )
            .await,
        Err(ClaimStoreError::OwnerNodeMismatch)
    ));
    let mut peer_request = original.clone();
    peer_request.node_id = "peer-node".into();
    assert!(matches!(
        fixture.store.delegate(&peer_request, later).await,
        Err(ClaimStoreError::OwnerNodeMismatch)
    ));
    assert_eq!(fixture.snapshot("task").await, before);

    let owner = ClaimOwner {
        owner_id: "owner",
        node_id: &original.node_id,
    };
    assert!(fixture
        .store
        .park(&fixture.tenant, "repository", "task", owner, later)
        .await
        .unwrap());
    let parked = fixture.snapshot("task").await;
    assert!(matches!(
        fixture
            .store
            .answer(
                &fixture.tenant,
                "repository",
                "task",
                peer,
                Duration::from_secs(60),
                later
            )
            .await,
        Err(ClaimStoreError::OwnerNodeMismatch)
    ));
    assert_eq!(fixture.snapshot("task").await, parked);
    assert_eq!(
        fixture
            .store
            .answer(
                &fixture.tenant,
                "repository",
                "task",
                owner,
                Duration::from_secs(60),
                later
            )
            .await
            .unwrap()
            .outcome,
        ClaimLeaseOutcome::Granted
    );
}

#[tokio::test]
async fn concurrent_nodes_cannot_both_claim_the_same_owner_label() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    let first = request(&fixture.tenant, "task", "owner");
    let mut second = first.clone();
    second.node_id = "peer-node".into();
    let (first_result, second_result) = tokio::join!(
        fixture.store.delegate(&first, fixture.now),
        fixture.store.delegate(&second, fixture.now),
    );
    let winner = match (first_result, second_result) {
        (Ok(granted), Err(ClaimStoreError::OwnerNodeMismatch))
            if granted.outcome == ClaimLeaseOutcome::Granted =>
        {
            &first.node_id
        }
        (Err(ClaimStoreError::OwnerNodeMismatch), Ok(granted))
            if granted.outcome == ClaimLeaseOutcome::Granted =>
        {
            &second.node_id
        }
        other => panic!("one node must win and the peer must be refused: {other:?}"),
    };
    assert_eq!(fixture.snapshot("task").await["owner_node_id"], *winner);
}

#[tokio::test]
async fn expired_custody_transfer_resets_the_window_even_with_the_same_owner_label() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    for recover in [false, true] {
        let task = if recover { "recovered" } else { "delegated" };
        let original = request(&fixture.tenant, task, "owner");
        fixture
            .store
            .delegate(&original, fixture.now)
            .await
            .unwrap();
        let later = fixture.now + Duration::from_secs(61);
        let transferred = if recover {
            let mut transfer = recover_request(
                &fixture.tenant,
                task,
                "owner",
                "owner",
                "prior node lost custody",
            );
            transfer.node_id = "peer-node".into();
            transfer.branch = "agents/peer".into();
            fixture.store.recover(&transfer, later).await.unwrap()
        } else {
            let mut transfer = original.clone();
            transfer.node_id = "peer-node".into();
            transfer.branch = "agents/peer".into();
            fixture.store.delegate(&transfer, later).await.unwrap()
        };
        assert_eq!(transferred.outcome, ClaimLeaseOutcome::Granted);
        assert_eq!(transferred.claim_started_at, later);
        assert_eq!(transferred.branch, "agents/peer");
        assert_eq!(transferred.claim_lapses, 1);
        assert_eq!(fixture.snapshot(task).await["owner_node_id"], "peer-node");
        let recorded: String = fixture.pool.get().await.unwrap().query_one(
            "SELECT requested_node_id FROM delegated_claim_history WHERE tenant_id = $1 AND task_id = $2 ORDER BY history_id DESC LIMIT 1",
            &[&fixture.tenant, &task],
        ).await.unwrap().get(0);
        assert_eq!(recorded, "peer-node");
    }
}

#[tokio::test]
async fn unknown_legacy_custody_is_not_adopted_by_an_owner_mutation() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    let original = request(&fixture.tenant, "legacy", "owner");
    fixture
        .store
        .delegate(&original, fixture.now)
        .await
        .unwrap();
    fixture.pool.get().await.unwrap().execute(
        "UPDATE delegated_claims SET owner_node_id = NULL WHERE tenant_id = $1 AND task_id = 'legacy'",
        &[&fixture.tenant],
    ).await.unwrap();
    let owner = ClaimOwner {
        owner_id: "owner",
        node_id: &original.node_id,
    };
    let later = fixture.now + Duration::from_secs(1);
    let before = fixture.snapshot("legacy").await;
    assert!(matches!(
        fixture
            .store
            .release(&fixture.tenant, "repository", "legacy", owner, later)
            .await,
        Err(ClaimStoreError::OwnerNodeMismatch)
    ));
    assert!(matches!(
        fixture
            .store
            .renew(
                &fixture.tenant,
                "repository",
                "legacy",
                owner,
                Duration::from_secs(60),
                later
            )
            .await,
        Err(ClaimStoreError::OwnerNodeMismatch)
    ));
    assert!(matches!(
        fixture.store.delegate(&original, later).await,
        Err(ClaimStoreError::OwnerNodeMismatch)
    ));
    assert_eq!(fixture.snapshot("legacy").await, before);
    let fresh_at = fixture.now + Duration::from_secs(61);
    let fresh = fixture.store.delegate(&original, fresh_at).await.unwrap();
    assert_eq!(fresh.claim_started_at, fresh_at);
    assert_eq!(
        fixture.snapshot("legacy").await["owner_node_id"],
        original.node_id
    );
}

#[tokio::test]
async fn node_custody_migration_preserves_legacy_rows_without_inventing_an_owner_node() {
    let Some(fixture) = Fixture::new().await else {
        return;
    };
    let mut connection = fixture.pool.get().await.unwrap();
    let transaction = connection.transaction().await.unwrap();
    let schema = format!("claim_node_upgrade_{}", crate::test_support::uuid_ish());
    assert!(schema
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'));
    transaction
        .batch_execute(&format!(
            "CREATE SCHEMA {schema}; SET LOCAL search_path TO {schema};"
        ))
        .await
        .unwrap();
    transaction.batch_execute(MIGRATION).await.unwrap();
    transaction.execute(
        "INSERT INTO delegated_claims (tenant_id, repository_id, task_id, owner_id, branch, claim_started_at, lease_expires_at, paths, symbols) VALUES ('tenant', 'repository', 'legacy', 'owner', 'old-branch', $1, $2, ARRAY['src/old.rs'], ARRAY[]::text[])",
        &[&fixture.now, &(fixture.now + Duration::from_secs(60))],
    ).await.unwrap();
    let before: String = transaction
        .query_one(
            "SELECT to_jsonb(claim)::text FROM delegated_claims claim",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    let mut before: serde_json::Value = serde_json::from_str(&before).unwrap();
    transaction
        .batch_execute(NODE_CUSTODY_MIGRATION)
        .await
        .unwrap();
    transaction
        .batch_execute(NODE_CUSTODY_MIGRATION)
        .await
        .unwrap();
    let after: String = transaction
        .query_one(
            "SELECT to_jsonb(claim)::text FROM delegated_claims claim",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    let after: serde_json::Value = serde_json::from_str(&after).unwrap();
    before["owner_node_id"] = serde_json::Value::Null;
    assert_eq!(before, after);
    transaction.rollback().await.unwrap();
}

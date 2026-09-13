use ackplane_client::{authenticate, SeedSigner};
use ackplane_protocol::v1::{
    claim_delegation_service_client::ClaimDelegationServiceClient, ClaimRecoverRequest,
};

use super::*;

pub(super) async fn assert_peer_refusals(
    pool: &ackplane_server::db_pool::PgPool,
    endpoint: &str,
    task_id: &str,
    owner_id: &str,
) {
    let peer_key = SigningKey::from_bytes(&[23; 32]);
    let key_id = format!("{task_id}:peer-key");
    let node_id = format!("{task_id}:peer-node");
    let mut database = pool.get().await.unwrap();
    let transaction = database.transaction().await.unwrap();
    signing_keys::register(
        &transaction,
        &SigningKeyRecord {
            signing_key_id: key_id.clone(),
            tenant_id: TENANT_ID.into(),
            repository_id: REPOSITORY_ID.into(),
            node_id: node_id.clone(),
            public_key: peer_key.verifying_key().to_bytes().to_vec(),
            public_key_fingerprint: key_id.clone(),
            activated_at: std::time::SystemTime::UNIX_EPOCH,
            expires_at: None,
        },
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    drop(database);
    let signer = SeedSigner::new(key_id, node_id.clone(), &[23; 32]);
    let mut peer = ClaimDelegationServiceClient::connect(endpoint.to_string())
        .await
        .unwrap();
    let other_task = format!("{task_id}:peer-own-task");
    let paths = vec!["src/lib.rs".to_string()];
    let delegate = ClaimOperation::Delegate {
        branch: "feat/owner-a",
        lease_seconds: 60,
        paths: &paths,
        symbols: &[],
    };
    let own_claim = peer
        .delegate_claim(ClaimLeaseRequest {
            tenant_id: TENANT_ID.into(),
            repository_id: REPOSITORY_ID.into(),
            task_id: other_task.clone(),
            owner_id: owner_id.into(),
            branch: "feat/owner-a".into(),
            lease_seconds: 60,
            paths: paths.clone(),
            symbols: vec![],
            authentication: Some(
                authenticate(
                    &signer,
                    TENANT_ID,
                    REPOSITORY_ID,
                    &other_task,
                    owner_id,
                    &delegate,
                )
                .unwrap(),
            ),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(
        own_claim.outcome(),
        ClaimLeaseOutcome::Granted,
        "the peer key really is enrolled and authorized for its own work"
    );
    let store = ClaimStore::connect(pool).await.unwrap();
    let before = store
        .list_active(TENANT_ID, REPOSITORY_ID, std::time::SystemTime::now())
        .await
        .unwrap()
        .into_iter()
        .find(|claim| claim.task_id == task_id)
        .unwrap();
    assert_eq!(before.node_id.as_deref(), Some(NODE_ID));

    for operation in [
        delegate,
        ClaimOperation::Renew { lease_seconds: 120 },
        ClaimOperation::Release,
        ClaimOperation::Park,
        ClaimOperation::Answer { lease_seconds: 120 },
    ] {
        let authentication = Some(
            authenticate(
                &signer,
                TENANT_ID,
                REPOSITORY_ID,
                task_id,
                owner_id,
                &operation,
            )
            .unwrap(),
        );
        let result = match operation {
            ClaimOperation::Delegate {
                branch,
                lease_seconds,
                paths,
                symbols,
            } => peer
                .delegate_claim(ClaimLeaseRequest {
                    tenant_id: TENANT_ID.into(),
                    repository_id: REPOSITORY_ID.into(),
                    task_id: task_id.into(),
                    owner_id: owner_id.into(),
                    branch: branch.into(),
                    lease_seconds,
                    paths: paths.into(),
                    symbols: symbols.into(),
                    authentication,
                })
                .await
                .map(|_| ()),
            ClaimOperation::Renew { lease_seconds } => peer
                .renew_claim(ClaimRenewRequest {
                    tenant_id: TENANT_ID.into(),
                    repository_id: REPOSITORY_ID.into(),
                    task_id: task_id.into(),
                    owner_id: owner_id.into(),
                    lease_seconds,
                    authentication,
                })
                .await
                .map(|_| ()),
            ClaimOperation::Release => peer
                .release_claim(ClaimReleaseRequest {
                    tenant_id: TENANT_ID.into(),
                    repository_id: REPOSITORY_ID.into(),
                    task_id: task_id.into(),
                    owner_id: owner_id.into(),
                    authentication,
                })
                .await
                .map(|_| ()),
            ClaimOperation::Park => peer
                .park_claim(ClaimParkRequest {
                    tenant_id: TENANT_ID.into(),
                    repository_id: REPOSITORY_ID.into(),
                    task_id: task_id.into(),
                    owner_id: owner_id.into(),
                    authentication,
                })
                .await
                .map(|_| ()),
            ClaimOperation::Answer { lease_seconds } => peer
                .answer_claim(ClaimAnswerRequest {
                    tenant_id: TENANT_ID.into(),
                    repository_id: REPOSITORY_ID.into(),
                    task_id: task_id.into(),
                    owner_id: owner_id.into(),
                    lease_seconds,
                    authentication,
                })
                .await
                .map(|_| ()),
            ClaimOperation::Recover { .. } => unreachable!("live recovery is checked separately"),
        };
        assert_eq!(
            result
                .expect_err("a peer cannot mutate the other node's owner")
                .code(),
            tonic::Code::PermissionDenied
        );
        let current = store
            .list_active(TENANT_ID, REPOSITORY_ID, std::time::SystemTime::now())
            .await
            .unwrap()
            .into_iter()
            .find(|claim| claim.task_id == task_id)
            .unwrap();
        assert_eq!(current, before);
    }
    let recover = ClaimOperation::Recover {
        expected_owner: owner_id,
        branch: "feat/peer",
        lease_seconds: 60,
        paths: &paths,
        symbols: &[],
        reason: "peer request must not take a live lease",
    };
    let recovered = peer
        .recover_claim(ClaimRecoverRequest {
            tenant_id: TENANT_ID.into(),
            repository_id: REPOSITORY_ID.into(),
            task_id: task_id.into(),
            expected_owner: owner_id.into(),
            owner_id: owner_id.into(),
            branch: "feat/peer".into(),
            lease_seconds: 60,
            paths: paths.clone(),
            symbols: vec![],
            reason: "peer request must not take a live lease".into(),
            authentication: Some(
                authenticate(
                    &signer,
                    TENANT_ID,
                    REPOSITORY_ID,
                    task_id,
                    owner_id,
                    &recover,
                )
                .unwrap(),
            ),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(recovered.outcome(), ClaimLeaseOutcome::Rejected);
    assert!(
        peer.park_claim(ClaimParkRequest {
            tenant_id: TENANT_ID.into(),
            repository_id: REPOSITORY_ID.into(),
            task_id: task_id.into(),
            owner_id: owner_id.into(),
            authentication: Some(authentication(
                TENANT_ID,
                REPOSITORY_ID,
                task_id,
                owner_id,
                &ClaimOperation::Park
            )),
        })
        .await
        .unwrap()
        .into_inner()
        .parked
    );
    let answer = ClaimOperation::Answer { lease_seconds: 60 };
    let refused = peer
        .answer_claim(ClaimAnswerRequest {
            tenant_id: TENANT_ID.into(),
            repository_id: REPOSITORY_ID.into(),
            task_id: task_id.into(),
            owner_id: owner_id.into(),
            lease_seconds: 60,
            authentication: Some(
                authenticate(
                    &signer,
                    TENANT_ID,
                    REPOSITORY_ID,
                    task_id,
                    owner_id,
                    &answer,
                )
                .unwrap(),
            ),
        })
        .await
        .unwrap_err();
    assert_eq!(refused.code(), tonic::Code::PermissionDenied);
    let answered = peer
        .answer_claim(ClaimAnswerRequest {
            tenant_id: TENANT_ID.into(),
            repository_id: REPOSITORY_ID.into(),
            task_id: task_id.into(),
            owner_id: owner_id.into(),
            lease_seconds: 60,
            authentication: Some(authentication(
                TENANT_ID,
                REPOSITORY_ID,
                task_id,
                owner_id,
                &answer,
            )),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(answered.outcome(), ClaimLeaseOutcome::Granted);
    assert!(
        peer.release_claim(ClaimReleaseRequest {
            tenant_id: TENANT_ID.into(),
            repository_id: REPOSITORY_ID.into(),
            task_id: other_task.clone(),
            owner_id: owner_id.into(),
            authentication: Some(
                authenticate(
                    &signer,
                    TENANT_ID,
                    REPOSITORY_ID,
                    &other_task,
                    owner_id,
                    &ClaimOperation::Release
                )
                .unwrap()
            ),
        })
        .await
        .unwrap()
        .into_inner()
        .released
    );
}

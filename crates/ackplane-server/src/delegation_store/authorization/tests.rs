use std::time::{Duration, SystemTime};

use ackplane_protocol::delegation::DelegatedAction;
use tokio_postgres::{Client, NoTls};

use super::*;
use crate::delegation_store::{
    DelegationGrantRequest, DelegationProjection, DelegationRevocationRequest, DelegationStore,
};

fn unique_scope(label: &str) -> (String, String) {
    let mut bytes = [0_u8; 8];
    getrandom::getrandom(&mut bytes).expect("the OS random source should be available");
    let suffix: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    (
        format!("tenant-delegation-use-{label}-{suffix}"),
        format!("repository-delegation-use-{label}-{suffix}"),
    )
}

fn grant_request(
    tenant_id: String,
    repository_id: String,
    label: &str,
    max_token_budget: u32,
    max_actions_per_session: u32,
    effective_at: SystemTime,
) -> DelegationGrantRequest {
    DelegationGrantRequest {
        tenant_id,
        repository_id,
        verified_issuer_principal_id: format!("principal:human-{label}"),
        delegatee_session_id: format!("session:v1:agent-{label}"),
        project_id: Some(format!("project:{label}")),
        task_id: Some(format!("task:{label}")),
        goal_id: format!("goal:{label}"),
        goal_digest: vec![1; 32],
        policy_version: "policy:v1".to_string(),
        policy_digest: vec![2; 32],
        constitution_version: "constitution:v1".to_string(),
        constitution_digest: vec![3; 32],
        allowed_actions: vec![
            DelegatedAction::RetrieveContext,
            DelegatedAction::RunValidation,
        ],
        max_token_budget,
        max_actions_per_session,
        source_protocol_version: 1,
        effective_at,
        expires_at: effective_at + Duration::from_secs(600),
        idempotency_key: format!("delegation:grant:{label}"),
    }
}

fn use_request(
    tenant_id: &str,
    repository_id: &str,
    projection: &DelegationProjection,
    action: DelegatedAction,
    reserved_token_budget: u32,
    idempotency_key: &str,
) -> DelegationUseRequest {
    DelegationUseRequest {
        tenant_id: tenant_id.to_string(),
        repository_id: repository_id.to_string(),
        delegation_id: projection.delegation_id.clone(),
        delegatee_session_id: projection.delegatee_session_id.clone(),
        project_id: projection.project_id.clone(),
        task_id: projection.task_id.clone(),
        goal_id: projection.goal_id.clone(),
        policy_version: projection.policy_version.clone(),
        policy_digest: projection.policy_digest.clone(),
        constitution_version: projection.constitution_version.clone(),
        constitution_digest: projection.constitution_digest.clone(),
        action,
        reserved_token_budget,
        idempotency_key: idempotency_key.to_string(),
    }
}

async fn store() -> Option<DelegationStore> {
    let database_url = std::env::var("ACKPLANE_TEST_DATABASE_URL").ok()?;
    Some(
        DelegationStore::connect(&database_url)
            .await
            .expect("the test database should accept delegation connections"),
    )
}

async fn database_client(database_url: &str) -> Client {
    let (client, connection) = tokio_postgres::connect(database_url, NoTls)
        .await
        .expect("the test database should accept a PostgreSQL connection");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn wait_for_projection_lock_waiters(monitor: &Client) {
    for _ in 0..200 {
        let count: i64 = monitor
            .query_one(
                "SELECT COUNT(*) \
                 FROM pg_stat_activity \
                 WHERE datname = current_database() \
                   AND wait_event_type = 'Lock' \
                   AND query LIKE '%FROM delegation_projections%' \
                   AND query LIKE '%FOR UPDATE%'",
                &[],
            )
            .await
            .expect("inspect PostgreSQL authorization lock waiters")
            .get(0);
        if count >= 2 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("both authorization attempts should wait on the delegation projection");
}

#[tokio::test]
async fn authorization_records_an_immutable_receipt_and_replays_only_identical_input() {
    let Some(mut store) = store().await else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let (tenant_id, repository_id) = unique_scope("replay");
    let granted = store
        .grant(grant_request(
            tenant_id.clone(),
            repository_id.clone(),
            "replay",
            16,
            2,
            SystemTime::now() - Duration::from_secs(1),
        ))
        .await
        .expect("grant delegation");
    let request = use_request(
        &tenant_id,
        &repository_id,
        &granted.projection,
        DelegatedAction::RetrieveContext,
        5,
        "delegation:use:replay",
    );

    let authorized = store
        .authorize_use(request.clone(), SystemTime::now())
        .await
        .expect("authorize the declared routine action");
    assert!(!authorized.idempotent_replay);
    assert_eq!(authorized.receipt.status, DelegationUseStatus::Authorized);
    assert_eq!(authorized.receipt.refusal_reason, None);
    assert_eq!(
        authorized.receipt.issuer_principal_id,
        granted.projection.issuer_principal_id
    );
    assert_eq!(authorized.receipt.delegation_version, 1);

    let replay = store
        .authorize_use(request.clone(), SystemTime::now())
        .await
        .expect("replay exact delegation use");
    assert!(replay.idempotent_replay);
    assert_eq!(replay.receipt, authorized.receipt);

    let mut changed = request;
    changed.reserved_token_budget = 6;
    assert!(matches!(
        store.authorize_use(changed, SystemTime::now()).await,
        Err(DelegationUseError::IdempotencyConflict)
    ));

    let page = store
        .list_use_receipts(
            &tenant_id,
            &repository_id,
            &granted.projection.delegation_id,
            None,
            10,
        )
        .await
        .expect("list durable delegation-use receipts");
    assert_eq!(page.entries, vec![authorized.receipt]);
    assert_eq!(page.next_after, None);
}

// Regression: checking idempotency before serialising on the projection let
// concurrent identical requests both miss the receipt, so one surfaced a
// unique-key error instead of replaying the other request's immutable receipt.
#[tokio::test]
async fn concurrent_identical_authorization_replays_after_the_projection_lock() {
    let Some(mut store) = store().await else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let database_url =
        std::env::var("ACKPLANE_TEST_DATABASE_URL").expect("test URL was used to open the store");
    let (tenant_id, repository_id) = unique_scope("concurrent-replay");
    let granted = store
        .grant(grant_request(
            tenant_id.clone(),
            repository_id.clone(),
            "concurrent-replay",
            16,
            2,
            SystemTime::now() - Duration::from_secs(1),
        ))
        .await
        .expect("grant delegation");
    let request = use_request(
        &tenant_id,
        &repository_id,
        &granted.projection,
        DelegatedAction::RetrieveContext,
        5,
        "delegation:use:concurrent-replay",
    );

    let mut holder = database_client(&database_url).await;
    let holder_transaction = holder
        .transaction()
        .await
        .expect("begin projection lock transaction");
    holder_transaction
        .query_one(
            "SELECT 1 FROM delegation_projections \
             WHERE tenant_id = $1 AND repository_id = $2 AND delegation_id = $3 \
             FOR UPDATE",
            &[
                &tenant_id,
                &repository_id,
                &granted.projection.delegation_id,
            ],
        )
        .await
        .expect("lock delegation projection");

    let mut first = DelegationStore::connect(&database_url)
        .await
        .expect("open first authorization connection");
    let mut second = DelegationStore::connect(&database_url)
        .await
        .expect("open second authorization connection");
    let now = SystemTime::now();
    let first_request = request.clone();
    let second_request = request.clone();
    let first_authorization =
        tokio::spawn(async move { first.authorize_use(first_request, now).await });
    let second_authorization =
        tokio::spawn(async move { second.authorize_use(second_request, now).await });

    let monitor = database_client(&database_url).await;
    wait_for_projection_lock_waiters(&monitor).await;
    holder_transaction
        .commit()
        .await
        .expect("release delegation projection lock");

    let first = first_authorization
        .await
        .expect("first authorization task should not panic")
        .expect("first authorization should succeed");
    let second = second_authorization
        .await
        .expect("second authorization task should not panic")
        .expect("second authorization should replay");
    assert_ne!(first.idempotent_replay, second.idempotent_replay);
    assert_eq!(first.receipt, second.receipt);

    let receipts = store
        .list_use_receipts(
            &tenant_id,
            &repository_id,
            &granted.projection.delegation_id,
            None,
            10,
        )
        .await
        .expect("list durable delegation-use receipts");
    assert_eq!(receipts.entries, vec![first.receipt]);
}

#[tokio::test]
async fn authorization_refuses_session_scope_and_basis_mismatches_without_consuming_limits() {
    let Some(mut store) = store().await else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let (tenant_id, repository_id) = unique_scope("basis");
    let granted = store
        .grant(grant_request(
            tenant_id.clone(),
            repository_id.clone(),
            "basis",
            3,
            1,
            SystemTime::now() - Duration::from_secs(1),
        ))
        .await
        .expect("grant delegation");

    let mut wrong_session = use_request(
        &tenant_id,
        &repository_id,
        &granted.projection,
        DelegatedAction::RetrieveContext,
        0,
        "delegation:use:wrong-session",
    );
    wrong_session.delegatee_session_id = "session:v1:someone-else".to_string();
    let wrong_session = store
        .authorize_use(wrong_session, SystemTime::now())
        .await
        .expect("session mismatch is a durable refusal");
    assert_eq!(wrong_session.receipt.status, DelegationUseStatus::Refused);
    assert_eq!(
        wrong_session.receipt.refusal_reason,
        Some(DelegationUseRefusal::DelegateeSessionMismatch)
    );

    let mut wrong_scope = use_request(
        &tenant_id,
        &repository_id,
        &granted.projection,
        DelegatedAction::RetrieveContext,
        0,
        "delegation:use:wrong-scope",
    );
    wrong_scope.task_id = Some("task:outside-envelope".to_string());
    let wrong_scope = store
        .authorize_use(wrong_scope, SystemTime::now())
        .await
        .expect("scope mismatch is a durable refusal");
    assert_eq!(
        wrong_scope.receipt.refusal_reason,
        Some(DelegationUseRefusal::ScopeMismatch)
    );

    let mut wrong_policy = use_request(
        &tenant_id,
        &repository_id,
        &granted.projection,
        DelegatedAction::RetrieveContext,
        0,
        "delegation:use:wrong-policy",
    );
    wrong_policy.policy_digest = vec![9; 32];
    let wrong_policy = store
        .authorize_use(wrong_policy, SystemTime::now())
        .await
        .expect("policy mismatch is a durable refusal");
    assert_eq!(
        wrong_policy.receipt.refusal_reason,
        Some(DelegationUseRefusal::PolicyBasisMismatch)
    );

    let mut wrong_constitution = use_request(
        &tenant_id,
        &repository_id,
        &granted.projection,
        DelegatedAction::RetrieveContext,
        0,
        "delegation:use:wrong-constitution",
    );
    wrong_constitution.constitution_digest = vec![8; 32];
    let wrong_constitution = store
        .authorize_use(wrong_constitution, SystemTime::now())
        .await
        .expect("Constitution mismatch is a durable refusal");
    assert_eq!(
        wrong_constitution.receipt.refusal_reason,
        Some(DelegationUseRefusal::ConstitutionBasisMismatch)
    );

    let mut restricted_grant = grant_request(
        tenant_id.clone(),
        repository_id.clone(),
        "restricted",
        3,
        1,
        SystemTime::now() - Duration::from_secs(1),
    );
    restricted_grant.allowed_actions = vec![DelegatedAction::RetrieveContext];
    let restricted = store
        .grant(restricted_grant)
        .await
        .expect("grant restricted delegation");
    let action_not_allowed = store
        .authorize_use(
            use_request(
                &tenant_id,
                &repository_id,
                &restricted.projection,
                DelegatedAction::RunValidation,
                0,
                "delegation:use:action-not-allowed",
            ),
            SystemTime::now(),
        )
        .await
        .expect("disallowed action is a durable refusal");
    assert_eq!(
        action_not_allowed.receipt.refusal_reason,
        Some(DelegationUseRefusal::ActionNotAllowed)
    );

    let token_exceeded = store
        .authorize_use(
            use_request(
                &tenant_id,
                &repository_id,
                &granted.projection,
                DelegatedAction::RetrieveContext,
                4,
                "delegation:use:token-limit",
            ),
            SystemTime::now(),
        )
        .await
        .expect("token limit is a durable refusal");
    assert_eq!(
        token_exceeded.receipt.refusal_reason,
        Some(DelegationUseRefusal::TokenBudgetExceeded)
    );

    let authorized = store
        .authorize_use(
            use_request(
                &tenant_id,
                &repository_id,
                &granted.projection,
                DelegatedAction::RetrieveContext,
                3,
                "delegation:use:authorized",
            ),
            SystemTime::now(),
        )
        .await
        .expect("refused uses must not consume the budget");
    assert_eq!(authorized.receipt.status, DelegationUseStatus::Authorized);

    let action_exceeded = store
        .authorize_use(
            use_request(
                &tenant_id,
                &repository_id,
                &granted.projection,
                DelegatedAction::RunValidation,
                0,
                "delegation:use:action-limit",
            ),
            SystemTime::now(),
        )
        .await
        .expect("action limit is a durable refusal");
    assert_eq!(
        action_exceeded.receipt.refusal_reason,
        Some(DelegationUseRefusal::ActionLimitExceeded)
    );
}

#[tokio::test]
async fn revoked_or_not_yet_effective_delegations_are_safely_refused() {
    let Some(mut store) = store().await else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let (tenant_id, repository_id) = unique_scope("lifecycle");
    let effective_at = SystemTime::now() + Duration::from_secs(60);
    let future = store
        .grant(grant_request(
            tenant_id.clone(),
            repository_id.clone(),
            "future",
            8,
            2,
            effective_at,
        ))
        .await
        .expect("grant future delegation");
    let not_yet_effective = store
        .authorize_use(
            use_request(
                &tenant_id,
                &repository_id,
                &future.projection,
                DelegatedAction::RetrieveContext,
                0,
                "delegation:use:not-effective",
            ),
            SystemTime::now(),
        )
        .await
        .expect("future delegation refusal");
    assert_eq!(
        not_yet_effective.receipt.refusal_reason,
        Some(DelegationUseRefusal::NotYetEffective)
    );

    let expired = store
        .grant(grant_request(
            tenant_id.clone(),
            repository_id.clone(),
            "expired",
            8,
            2,
            SystemTime::now() - Duration::from_secs(1),
        ))
        .await
        .expect("grant delegation that will be evaluated after expiry");
    let expired_use = store
        .authorize_use(
            use_request(
                &tenant_id,
                &repository_id,
                &expired.projection,
                DelegatedAction::RetrieveContext,
                0,
                "delegation:use:expired",
            ),
            expired.projection.expires_at + Duration::from_secs(1),
        )
        .await
        .expect("expired delegation refusal");
    assert_eq!(
        expired_use.receipt.refusal_reason,
        Some(DelegationUseRefusal::Expired)
    );

    let active = store
        .grant(grant_request(
            tenant_id.clone(),
            repository_id.clone(),
            "revoked",
            8,
            2,
            SystemTime::now() - Duration::from_secs(1),
        ))
        .await
        .expect("grant active delegation");
    store
        .revoke(DelegationRevocationRequest {
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            delegation_id: active.projection.delegation_id.clone(),
            verified_revoker_principal_id: "principal:human-revoker".to_string(),
            reason: "delegation no longer approved".to_string(),
            expected_version: active.projection.version,
            idempotency_key: "delegation:revoke:lifecycle".to_string(),
        })
        .await
        .expect("revoke delegation");
    let revoked = store
        .authorize_use(
            use_request(
                &tenant_id,
                &repository_id,
                &active.projection,
                DelegatedAction::RetrieveContext,
                0,
                "delegation:use:revoked",
            ),
            SystemTime::now(),
        )
        .await
        .expect("revoked delegation refusal");
    assert_eq!(
        revoked.receipt.refusal_reason,
        Some(DelegationUseRefusal::Revoked)
    );
}

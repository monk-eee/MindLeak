use std::time::{Duration, SystemTime};

use super::{model::AdministrationStoreError, *};
use crate::test_support::unique_id;

fn database_url() -> Option<String> {
    std::env::var("ACKPLANE_TEST_DATABASE_URL").ok()
}

fn policy_request(
    operation: AdministrationOperation,
    adopted_by: &str,
    suffix: &str,
) -> PolicyAdoptionRequest {
    let now = SystemTime::now();
    PolicyAdoptionRequest {
        operation,
        scope: AdministrationScope::Platform,
        data_classification: "operational-metadata".to_owned(),
        retention_basis: "self-hosted operator retention, ADR-0119 decision 2".to_owned(),
        adopted_by: adopted_by.to_owned(),
        idempotency_key: format!("policy-{suffix}"),
        effective_at: now,
        expires_at: now + Duration::from_secs(3600),
    }
}

#[tokio::test]
async fn an_identical_policy_request_replays_its_original_record() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-policy-replay");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let request = policy_request(
        AdministrationOperation::Snapshot,
        "loopback-principal",
        &suffix,
    );
    let first = store
        .adopt_policy(&request)
        .await
        .expect("the first adoption should succeed");
    assert!(!first.idempotent_replay);

    let second = store
        .adopt_policy(&request)
        .await
        .expect("the exact same request should replay rather than fail");
    assert!(second.idempotent_replay);
    assert_eq!(first.policy.policy_id, second.policy.policy_id);
}

#[tokio::test]
async fn a_changed_policy_request_under_the_same_idempotency_key_conflicts() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-policy-conflict");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let mut request = policy_request(
        AdministrationOperation::Snapshot,
        "loopback-principal",
        &suffix,
    );
    store
        .adopt_policy(&request)
        .await
        .expect("the first adoption should succeed");

    request.retention_basis = "a different retention basis entirely".to_owned();
    let result = store.adopt_policy(&request).await;
    assert!(matches!(
        result,
        Err(AdministrationStoreError::PolicyIdempotencyConflict)
    ));
}

#[tokio::test]
async fn a_snapshot_request_with_no_active_policy_is_refused() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-snapshot-no-policy");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let result = store
        .request_snapshot(
            &NewSnapshotRequest {
                policy_id: format!("administration-policy:does-not-exist-{suffix}"),
                requested_by: "loopback-principal".to_owned(),
                scope: AdministrationScope::Platform,
                idempotency_key: format!("snapshot-{suffix}"),
            },
            SystemTime::now(),
        )
        .await;
    assert!(matches!(
        result,
        Err(AdministrationStoreError::NoActivePolicy)
    ));
}

#[tokio::test]
async fn a_snapshot_request_under_an_active_policy_succeeds_and_replays() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-snapshot-active-policy");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let policy = store
        .adopt_policy(&policy_request(
            AdministrationOperation::Snapshot,
            "loopback-principal",
            &suffix,
        ))
        .await
        .expect("policy adoption should succeed")
        .policy;

    let new_request = NewSnapshotRequest {
        policy_id: policy.policy_id.clone(),
        requested_by: "loopback-principal".to_owned(),
        scope: AdministrationScope::Platform,
        idempotency_key: format!("snapshot-{suffix}"),
    };
    let first = store
        .request_snapshot(&new_request, SystemTime::now())
        .await
        .expect("a request under an active policy should succeed");
    assert!(!first.idempotent_replay);

    let second = store
        .request_snapshot(&new_request, SystemTime::now())
        .await
        .expect("the exact same request should replay");
    assert!(second.idempotent_replay);
    assert_eq!(first.request.request_id, second.request.request_id);
}

#[tokio::test]
async fn an_expired_policy_no_longer_authorizes_a_snapshot_request() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-snapshot-expired-policy");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let now = SystemTime::now();
    let mut request = policy_request(
        AdministrationOperation::Snapshot,
        "loopback-principal",
        &suffix,
    );
    request.effective_at = now - Duration::from_secs(7200);
    request.expires_at = now - Duration::from_secs(3600);
    let policy = store
        .adopt_policy(&request)
        .await
        .expect("adopting an already-expired policy record is itself allowed")
        .policy;

    let result = store
        .request_snapshot(
            &NewSnapshotRequest {
                policy_id: policy.policy_id,
                requested_by: "loopback-principal".to_owned(),
                scope: AdministrationScope::Platform,
                idempotency_key: format!("snapshot-{suffix}"),
            },
            now,
        )
        .await;
    assert!(matches!(
        result,
        Err(AdministrationStoreError::NoActivePolicy)
    ));
}

#[tokio::test]
async fn a_snapshot_receipt_replays_and_conflicts_like_every_other_immutable_receipt() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-snapshot-receipt");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let policy = store
        .adopt_policy(&policy_request(
            AdministrationOperation::Snapshot,
            "loopback-principal",
            &suffix,
        ))
        .await
        .expect("policy adoption should succeed")
        .policy;
    let snapshot_request = store
        .request_snapshot(
            &NewSnapshotRequest {
                policy_id: policy.policy_id,
                requested_by: "loopback-principal".to_owned(),
                scope: AdministrationScope::Platform,
                idempotency_key: format!("snapshot-{suffix}"),
            },
            SystemTime::now(),
        )
        .await
        .expect("the request should succeed")
        .request;

    let receipt = NewSnapshotReceipt {
        request_id: snapshot_request.request_id.clone(),
        outcome: SnapshotOutcome::Succeeded,
        reason: "pg_dump completed and the artifact was encrypted and verified.".to_owned(),
        artifact_path: Some(format!("/snapshots/{suffix}.dump.enc")),
        manifest_digest: Some(vec![9; 32]),
        encryption_key_id: Some("snapshot-key-v1".to_owned()),
        size_bytes: Some(4096),
        verified: true,
        occurred_at: SystemTime::now(),
    };

    let first = store
        .record_snapshot_receipt(&receipt, SystemTime::now())
        .await
        .expect("the first receipt should be recorded");
    let second = store
        .record_snapshot_receipt(&receipt, SystemTime::now())
        .await
        .expect("the exact same receipt should replay rather than fail");
    assert_eq!(first.receipt_id, second.receipt_id);

    let mut conflicting = receipt;
    conflicting.reason = "a different reason entirely".to_owned();
    let result = store
        .record_snapshot_receipt(&conflicting, SystemTime::now())
        .await;
    assert!(matches!(
        result,
        Err(AdministrationStoreError::ReceiptConflict)
    ));

    let fetched = store
        .snapshot_receipt_for_request(&snapshot_request.request_id)
        .await
        .expect("the receipt should be readable back by request id")
        .expect("a receipt was recorded for this request");
    assert_eq!(fetched.receipt_id, first.receipt_id);
    assert!(fetched.verified);
}

#[tokio::test]
async fn a_revoked_policy_no_longer_authorizes_a_snapshot_request() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-snapshot-revoked-policy");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let policy = store
        .adopt_policy(&policy_request(
            AdministrationOperation::Snapshot,
            "loopback-principal",
            &suffix,
        ))
        .await
        .expect("policy adoption should succeed")
        .policy;
    store
        .client
        .execute(
            "UPDATE administration_policies SET revoked_at = now(), revoked_by = $1 \
             WHERE policy_id = $2",
            &[&"loopback-principal".to_owned(), &policy.policy_id],
        )
        .await
        .expect("revoking the policy directly should succeed");

    let result = store
        .request_snapshot(
            &NewSnapshotRequest {
                policy_id: policy.policy_id,
                requested_by: "loopback-principal".to_owned(),
                scope: AdministrationScope::Platform,
                idempotency_key: format!("snapshot-{suffix}"),
            },
            SystemTime::now(),
        )
        .await;
    assert!(matches!(
        result,
        Err(AdministrationStoreError::NoActivePolicy)
    ));
}

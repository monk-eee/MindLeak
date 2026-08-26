use std::time::SystemTime;

use super::*;
use crate::test_support::unique_id;

fn database_url() -> Option<String> {
    std::env::var("ACKPLANE_TEST_DATABASE_URL").ok()
}

fn policy_request(adopted_by: &str, suffix: &str) -> PolicyAdoptionRequest {
    let now = SystemTime::now();
    PolicyAdoptionRequest {
        operation: AdministrationOperation::Snapshot,
        scope: AdministrationScope::Platform,
        data_classification: "operational-metadata".to_owned(),
        retention_basis: "self-hosted operator retention, ADR-0119 decision 2".to_owned(),
        adopted_by: adopted_by.to_owned(),
        idempotency_key: format!("policy-{suffix}"),
        effective_at: now,
        expires_at: now + std::time::Duration::from_secs(3600),
    }
}

#[tokio::test]
async fn recording_an_inspection_requires_an_existing_snapshot_request() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-recovery-inspection-unknown-request");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let now = SystemTime::now();
    let result = store
        .record_recovery_inspection(
            &NewRecoveryInspection {
                request_id: format!("administration-snapshot-request:does-not-exist-{suffix}"),
                requested_by: "loopback-principal".to_owned(),
                integrity_verified: true,
                decryption_verified: true,
                archive_valid: true,
                archive_entry_count: Some(10),
                reason: "pg_restore --list reported 10 archive entries.".to_owned(),
                occurred_at: now,
            },
            now,
        )
        .await;
    assert!(matches!(result, Err(AdministrationStoreError::Database(_))));
}

#[tokio::test]
async fn each_inspection_call_appends_its_own_immutable_record() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-recovery-inspection");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let policy = store
        .adopt_policy(&policy_request("loopback-principal", &suffix))
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
        .expect("the snapshot request should succeed")
        .request;

    let now = SystemTime::now();
    let first = store
        .record_recovery_inspection(
            &NewRecoveryInspection {
                request_id: snapshot_request.request_id.clone(),
                requested_by: "loopback-principal".to_owned(),
                integrity_verified: true,
                decryption_verified: true,
                archive_valid: true,
                archive_entry_count: Some(10),
                reason: "pg_restore --list reported 10 archive entries.".to_owned(),
                occurred_at: now,
            },
            now,
        )
        .await
        .expect("recording the first inspection should succeed");

    let second = store
        .record_recovery_inspection(
            &NewRecoveryInspection {
                request_id: snapshot_request.request_id.clone(),
                requested_by: "loopback-principal".to_owned(),
                integrity_verified: false,
                decryption_verified: false,
                archive_valid: false,
                archive_entry_count: None,
                reason: "The artifact's digest no longer matches its recorded manifest digest."
                    .to_owned(),
                occurred_at: now + std::time::Duration::from_secs(1),
            },
            now + std::time::Duration::from_secs(1),
        )
        .await
        .expect("recording a second, differently-outcomed inspection should also succeed");

    assert_ne!(first.inspection_id, second.inspection_id);

    let latest = store
        .latest_recovery_inspection(&snapshot_request.request_id)
        .await
        .expect("reading the latest inspection should succeed")
        .expect("an inspection was recorded for this request");
    assert_eq!(latest.inspection_id, second.inspection_id);
    assert!(!latest.integrity_verified);
}

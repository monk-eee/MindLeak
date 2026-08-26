use std::time::{Duration, SystemTime};

use super::{model::AdministrationStoreError, *};
use crate::test_support::unique_id;

fn database_url() -> Option<String> {
    std::env::var("ACKPLANE_TEST_DATABASE_URL").ok()
}

fn export_policy_request(adopted_by: &str, tenant_id: &str, suffix: &str) -> PolicyAdoptionRequest {
    let now = SystemTime::now();
    PolicyAdoptionRequest {
        operation: AdministrationOperation::Export,
        scope: AdministrationScope::Tenant(tenant_id.to_string()),
        data_classification: "diagnostic-telemetry".to_owned(),
        retention_basis: "named audit export, ADR-0119 decision 5".to_owned(),
        adopted_by: adopted_by.to_owned(),
        idempotency_key: format!("export-policy-{suffix}"),
        effective_at: now,
        expires_at: now + Duration::from_secs(3600),
    }
}

fn export_request(
    policy_id: &str,
    requested_by: &str,
    tenant_id: &str,
    repository_id: &str,
    suffix: &str,
) -> NewExportRequest {
    NewExportRequest {
        policy_id: policy_id.to_owned(),
        requested_by: requested_by.to_owned(),
        tenant_id: tenant_id.to_owned(),
        repository_id: repository_id.to_owned(),
        data_category: ExportDataCategory::TelemetryEvents,
        purpose: "quarterly diagnostic telemetry audit".to_owned(),
        max_records: 100,
        idempotency_key: format!("export-request-{suffix}"),
    }
}

#[tokio::test]
async fn an_export_request_with_no_active_policy_is_refused() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-export-no-policy");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let result = store
        .request_export(
            &export_request(
                &format!("administration-policy:does-not-exist-{suffix}"),
                "loopback-principal",
                &format!("tenant-{suffix}"),
                &format!("repository-{suffix}"),
                &suffix,
            ),
            SystemTime::now(),
        )
        .await;
    assert!(matches!(
        result,
        Err(AdministrationStoreError::NoActivePolicy)
    ));
}

#[tokio::test]
async fn an_export_request_under_an_active_policy_succeeds_and_replays() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-export-active-policy");
    let tenant_id = format!("tenant-{suffix}");
    let repository_id = format!("repository-{suffix}");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let policy = store
        .adopt_policy(&export_policy_request(
            "loopback-principal",
            &tenant_id,
            &suffix,
        ))
        .await
        .expect("policy adoption should succeed")
        .policy;

    let new_request = export_request(
        &policy.policy_id,
        "loopback-principal",
        &tenant_id,
        &repository_id,
        &suffix,
    );
    let first = store
        .request_export(&new_request, SystemTime::now())
        .await
        .expect("a request under an active policy should succeed");
    assert!(!first.idempotent_replay);
    assert_eq!(first.request.max_records, 100);

    let second = store
        .request_export(&new_request, SystemTime::now())
        .await
        .expect("the exact same request should replay");
    assert!(second.idempotent_replay);
    assert_eq!(first.request.request_id, second.request.request_id);
}

#[tokio::test]
async fn a_changed_export_request_under_the_same_idempotency_key_conflicts() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-export-conflict");
    let tenant_id = format!("tenant-{suffix}");
    let repository_id = format!("repository-{suffix}");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let policy = store
        .adopt_policy(&export_policy_request(
            "loopback-principal",
            &tenant_id,
            &suffix,
        ))
        .await
        .expect("policy adoption should succeed")
        .policy;
    let mut request = export_request(
        &policy.policy_id,
        "loopback-principal",
        &tenant_id,
        &repository_id,
        &suffix,
    );
    store
        .request_export(&request, SystemTime::now())
        .await
        .expect("the first request should succeed");

    request.purpose = "a completely different purpose".to_owned();
    let result = store.request_export(&request, SystemTime::now()).await;
    assert!(matches!(
        result,
        Err(AdministrationStoreError::RequestIdempotencyConflict)
    ));
}

#[tokio::test]
async fn an_export_receipt_replays_and_conflicts_like_every_other_immutable_receipt() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-export-receipt");
    let tenant_id = format!("tenant-{suffix}");
    let repository_id = format!("repository-{suffix}");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let policy = store
        .adopt_policy(&export_policy_request(
            "loopback-principal",
            &tenant_id,
            &suffix,
        ))
        .await
        .expect("policy adoption should succeed")
        .policy;
    let export_request_record = store
        .request_export(
            &export_request(
                &policy.policy_id,
                "loopback-principal",
                &tenant_id,
                &repository_id,
                &suffix,
            ),
            SystemTime::now(),
        )
        .await
        .expect("the request should succeed")
        .request;

    let receipt = NewExportReceipt {
        request_id: export_request_record.request_id.clone(),
        outcome: ExportOutcome::Succeeded,
        reason: "The bounded, redacted export completed.".to_owned(),
        artifact_path: Some(format!("/exports/{suffix}.json")),
        manifest_digest: Some(vec![3; 32]),
        schema_version: "telemetry-export-v1".to_owned(),
        record_count: Some(5),
        redacted_fields: vec!["node_id".to_owned(), "agent_session_id".to_owned()],
        occurred_at: SystemTime::now(),
    };

    let first = store
        .record_export_receipt(&receipt, SystemTime::now())
        .await
        .expect("the first receipt should be recorded");
    let second = store
        .record_export_receipt(&receipt, SystemTime::now())
        .await
        .expect("the exact same receipt should replay rather than fail");
    assert_eq!(first.receipt_id, second.receipt_id);

    let mut conflicting = receipt;
    conflicting.record_count = Some(999);
    let result = store
        .record_export_receipt(&conflicting, SystemTime::now())
        .await;
    assert!(matches!(
        result,
        Err(AdministrationStoreError::ReceiptConflict)
    ));

    let fetched = store
        .export_receipt_for_request(&export_request_record.request_id)
        .await
        .expect("the receipt should be readable back by request id")
        .expect("a receipt was recorded for this request");
    assert_eq!(fetched.receipt_id, first.receipt_id);
    assert_eq!(fetched.redacted_fields, vec!["node_id", "agent_session_id"]);
}

#[tokio::test]
async fn a_revoked_policy_no_longer_authorizes_an_export_request() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-export-revoked-policy");
    let tenant_id = format!("tenant-{suffix}");
    let repository_id = format!("repository-{suffix}");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let policy = store
        .adopt_policy(&export_policy_request(
            "loopback-principal",
            &tenant_id,
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
        .request_export(
            &export_request(
                &policy.policy_id,
                "loopback-principal",
                &tenant_id,
                &repository_id,
                &suffix,
            ),
            SystemTime::now(),
        )
        .await;
    assert!(matches!(
        result,
        Err(AdministrationStoreError::NoActivePolicy)
    ));
}

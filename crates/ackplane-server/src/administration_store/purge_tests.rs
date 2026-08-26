use std::time::{Duration, SystemTime};

use super::{model::AdministrationStoreError, *};
use crate::test_support::unique_id;

fn database_url() -> Option<String> {
    std::env::var("ACKPLANE_TEST_DATABASE_URL").ok()
}

fn purge_policy_request(adopted_by: &str, tenant_id: &str, suffix: &str) -> PolicyAdoptionRequest {
    let now = SystemTime::now();
    PolicyAdoptionRequest {
        operation: AdministrationOperation::LifecyclePurge,
        scope: AdministrationScope::Tenant(tenant_id.to_string()),
        data_classification: "diagnostic-telemetry".to_owned(),
        retention_basis: "operator-defined retention window, ADR-0119 decision 7".to_owned(),
        adopted_by: adopted_by.to_owned(),
        idempotency_key: format!("purge-policy-{suffix}"),
        effective_at: now,
        expires_at: now + Duration::from_secs(3600),
    }
}

fn preview_request(
    policy_id: &str,
    requested_by: &str,
    tenant_id: &str,
    repository_id: &str,
    suffix: &str,
) -> PurgePreviewRequest {
    PurgePreviewRequest {
        policy_id: policy_id.to_owned(),
        requested_by: requested_by.to_owned(),
        tenant_id: tenant_id.to_owned(),
        repository_id: repository_id.to_owned(),
        data_category: PurgeDataCategory::TelemetryEvents,
        older_than: SystemTime::now(),
        confirmation_window: Duration::from_secs(900),
        idempotency_key: format!("purge-request-{suffix}"),
    }
}

async fn insert_telemetry_event(
    store: &mut AdministrationStore,
    tenant_id: &str,
    repository_id: &str,
    telemetry_id: &str,
    occurred_at: SystemTime,
) {
    store
        .client
        .execute(
            "INSERT INTO telemetry_events (tenant_id, repository_id, telemetry_id, node_id, \
                 kind, name, outcome, duration_ms, occurred_at) \
             VALUES ($1,$2,$3,'node',1,'name',1,0,$4)",
            &[&tenant_id, &repository_id, &telemetry_id, &occurred_at],
        )
        .await
        .expect("inserting a telemetry event fixture should succeed");
}

#[tokio::test]
async fn a_preview_with_no_active_purge_policy_is_refused() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-purge-no-policy");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let result = store
        .preview_purge(
            &preview_request(
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
async fn preview_counts_only_matching_rows_and_confirm_deletes_exactly_those() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-purge-preview-confirm");
    let tenant_id = format!("tenant-{suffix}");
    let repository_id = format!("repository-{suffix}");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let now = SystemTime::now();
    let cutoff = now - Duration::from_secs(3600);
    insert_telemetry_event(
        &mut store,
        &tenant_id,
        &repository_id,
        &format!("old-1-{suffix}"),
        cutoff - Duration::from_secs(60),
    )
    .await;
    insert_telemetry_event(
        &mut store,
        &tenant_id,
        &repository_id,
        &format!("old-2-{suffix}"),
        cutoff - Duration::from_secs(120),
    )
    .await;
    insert_telemetry_event(
        &mut store,
        &tenant_id,
        &repository_id,
        &format!("new-1-{suffix}"),
        now,
    )
    .await;
    // A different tenant's old event must never be counted or deleted by
    // this request.
    insert_telemetry_event(
        &mut store,
        &format!("foreign-{tenant_id}"),
        &repository_id,
        &format!("foreign-old-{suffix}"),
        cutoff - Duration::from_secs(60),
    )
    .await;

    let policy = store
        .adopt_policy(&purge_policy_request(
            "loopback-principal",
            &tenant_id,
            &suffix,
        ))
        .await
        .expect("policy adoption should succeed")
        .policy;

    let mut preview = preview_request(
        &policy.policy_id,
        "loopback-principal",
        &tenant_id,
        &repository_id,
        &suffix,
    );
    preview.older_than = cutoff;
    let query_now = SystemTime::now();
    let outcome = store
        .preview_purge(&preview, query_now)
        .await
        .expect("the preview should succeed under an active policy");
    assert!(!outcome.idempotent_replay);
    assert_eq!(outcome.request.preview_row_count, 2);

    let receipt = store
        .confirm_purge(&outcome.request.request_id, query_now)
        .await
        .expect("confirming within the window should succeed");
    assert!(matches!(receipt.outcome, PurgeOutcome::Succeeded));
    assert_eq!(receipt.rows_deleted, Some(2));

    // Confirming again must replay the same receipt, not delete a second
    // time (there is nothing left to delete, but the contract holds either
    // way: it must not attempt to).
    let replay = store
        .confirm_purge(&outcome.request.request_id, now)
        .await
        .expect("re-confirming an already-receipted request should replay");
    assert_eq!(replay.receipt_id, receipt.receipt_id);
    assert_eq!(replay.rows_deleted, Some(2));

    let remaining: i64 = store
        .client
        .query_one(
            "SELECT COUNT(*) FROM telemetry_events WHERE tenant_id = $1",
            &[&tenant_id],
        )
        .await
        .expect("counting remaining telemetry events should succeed")
        .get(0);
    assert_eq!(
        remaining, 1,
        "only the newer event for this tenant should remain"
    );

    let foreign_remaining: i64 = store
        .client
        .query_one(
            "SELECT COUNT(*) FROM telemetry_events WHERE tenant_id = $1",
            &[&format!("foreign-{tenant_id}")],
        )
        .await
        .expect("counting the foreign tenant's telemetry events should succeed")
        .get(0);
    assert_eq!(
        foreign_remaining, 1,
        "a purge scoped to one tenant must never touch another tenant's rows"
    );
}

#[tokio::test]
async fn confirming_after_the_window_expires_refuses_without_deleting() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-purge-expired-confirmation");
    let tenant_id = format!("tenant-{suffix}");
    let repository_id = format!("repository-{suffix}");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let now = SystemTime::now();
    let cutoff = now - Duration::from_secs(3600);
    insert_telemetry_event(
        &mut store,
        &tenant_id,
        &repository_id,
        &format!("old-{suffix}"),
        cutoff - Duration::from_secs(60),
    )
    .await;

    let policy = store
        .adopt_policy(&purge_policy_request(
            "loopback-principal",
            &tenant_id,
            &suffix,
        ))
        .await
        .expect("policy adoption should succeed")
        .policy;
    let mut preview = preview_request(
        &policy.policy_id,
        "loopback-principal",
        &tenant_id,
        &repository_id,
        &suffix,
    );
    preview.older_than = cutoff;
    preview.confirmation_window = Duration::from_secs(1);
    let query_now = SystemTime::now();
    let outcome = store
        .preview_purge(&preview, query_now)
        .await
        .expect("the preview should succeed");

    let receipt = store
        .confirm_purge(
            &outcome.request.request_id,
            query_now + Duration::from_secs(120),
        )
        .await
        .expect("confirming after expiry should still return a receipt, not an error");
    assert!(matches!(receipt.outcome, PurgeOutcome::Expired));
    assert_eq!(receipt.rows_deleted, None);

    let remaining: i64 = store
        .client
        .query_one(
            "SELECT COUNT(*) FROM telemetry_events WHERE tenant_id = $1",
            &[&tenant_id],
        )
        .await
        .expect("counting telemetry events should succeed")
        .get(0);
    assert_eq!(
        remaining, 1,
        "an expired confirmation must not delete anything"
    );
}

#[tokio::test]
async fn confirming_after_the_policy_is_revoked_refuses_without_deleting() {
    let Some(database_url) = database_url() else {
        eprintln!("skipping: ACKPLANE_TEST_DATABASE_URL is not set");
        return;
    };
    let suffix = unique_id("administration-purge-revoked-policy");
    let tenant_id = format!("tenant-{suffix}");
    let repository_id = format!("repository-{suffix}");
    let mut store = AdministrationStore::connect(&database_url)
        .await
        .expect("the test database should accept administration store connections");

    let now = SystemTime::now();
    let cutoff = now - Duration::from_secs(3600);
    insert_telemetry_event(
        &mut store,
        &tenant_id,
        &repository_id,
        &format!("old-{suffix}"),
        cutoff - Duration::from_secs(60),
    )
    .await;

    let policy = store
        .adopt_policy(&purge_policy_request(
            "loopback-principal",
            &tenant_id,
            &suffix,
        ))
        .await
        .expect("policy adoption should succeed")
        .policy;
    let mut preview = preview_request(
        &policy.policy_id,
        "loopback-principal",
        &tenant_id,
        &repository_id,
        &suffix,
    );
    preview.older_than = cutoff;
    let query_now = SystemTime::now();
    let outcome = store
        .preview_purge(&preview, query_now)
        .await
        .expect("the preview should succeed");

    store
        .client
        .execute(
            "UPDATE administration_policies SET revoked_at = now(), revoked_by = $1 \
             WHERE policy_id = $2",
            &[&"loopback-principal".to_owned(), &policy.policy_id],
        )
        .await
        .expect("revoking the policy directly should succeed");

    let receipt = store
        .confirm_purge(&outcome.request.request_id, query_now)
        .await
        .expect("confirming against a revoked policy should still return a receipt");
    assert!(matches!(receipt.outcome, PurgeOutcome::Refused));
    assert_eq!(receipt.rows_deleted, None);

    let remaining: i64 = store
        .client
        .query_one(
            "SELECT COUNT(*) FROM telemetry_events WHERE tenant_id = $1",
            &[&tenant_id],
        )
        .await
        .expect("counting telemetry events should succeed")
        .get(0);
    assert_eq!(
        remaining, 1,
        "a refused confirmation must not delete anything"
    );
}

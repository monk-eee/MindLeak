//! HTTP-level coverage for enrolled-key authenticated Lifecycle-purge actions.

use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use ackplane_bridge::administration::{administration_routes, AdministrationApiState};
use ackplane_client::auth::{authenticate_lifecycle_purge, ClaimSigner, SeedSigner};
use ackplane_protocol::purge_confirmation_auth::LifecyclePurgeOperation;
use ackplane_server::{
    administration_store::AdministrationStore,
    claim_store::ClaimStore,
    enrollment::{activation_challenge_bytes, public_key_fingerprint},
    enrollment_store::{
        ActivationChallengeRequest, EnrollmentActivation, EnrollmentApproval, EnrollmentStore,
        EnrollmentSubmission,
    },
    fleet::FleetStore,
};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use tower::ServiceExt;

const DATA_CATEGORY: &str = "telemetry_events";
const CONFIRMATION_WINDOW_SECONDS: u64 = 900;

fn unique_suffix() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the system clock should be after the Unix epoch")
        .as_nanos();
    let mut random_bytes = [0_u8; 8];
    getrandom::getrandom(&mut random_bytes).expect("the OS random source should be available");
    let random_suffix: String = random_bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{timestamp}-{random_suffix}")
}

async fn enroll_signer(
    database_url: &str,
    tenant_id: &str,
    repository_id: &str,
    suffix: &str,
    label: &str,
    seed: [u8; 32],
) -> SeedSigner {
    let signing_key = SigningKey::from_bytes(&seed);
    let public_key = signing_key.verifying_key().to_bytes();
    let node_id = format!("administration-purge-{label}-node-{suffix}");
    let signing_key_id = format!("administration-purge-{label}-signing-key-{suffix}");
    let submission = EnrollmentSubmission {
        request_id: format!("administration-purge-{label}-request-{suffix}"),
        tenant_id: tenant_id.to_string(),
        repository_id: repository_id.to_string(),
        proposed_node_id: node_id.clone(),
        display_name: format!("Administration purge integration {label}"),
        public_key: public_key.to_vec(),
        public_key_fingerprint: public_key_fingerprint(&public_key),
        requested_capabilities: vec!["synchronize".to_string()],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        expires_at: "2030-01-01T00:00:00Z".to_string(),
    };
    let request = ActivationChallengeRequest {
        request_id: submission.request_id.clone(),
        tenant_id: tenant_id.to_string(),
        repository_id: repository_id.to_string(),
        proposed_node_id: node_id.clone(),
        public_key_fingerprint: submission.public_key_fingerprint.clone(),
    };
    let now = SystemTime::now();
    let mut enrollment = EnrollmentStore::connect(database_url)
        .await
        .expect("the test database should accept enrollment connections");
    enrollment
        .submit(&submission)
        .await
        .expect("the repository enrollment should be recorded");
    enrollment
        .approve(&EnrollmentApproval {
            request_id: submission.request_id.clone(),
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            public_key_fingerprint: submission.public_key_fingerprint.clone(),
            approved_capabilities: submission.requested_capabilities.clone(),
            approved_by: "administration-purge-integration-administrator".to_string(),
        })
        .await
        .expect("the repository enrollment should be approved");
    let nonce: [u8; 32] = Sha256::digest(format!("{suffix}-{label}").as_bytes()).into();
    let challenge = enrollment
        .issue_challenge(&request, &nonce, now)
        .await
        .expect("the approved repository should receive an activation challenge");
    let signature = signing_key.sign(&activation_challenge_bytes(
        &challenge.nonce,
        &request.request_id,
        &request.tenant_id,
        &request.repository_id,
        &request.proposed_node_id,
        &request.public_key_fingerprint,
    ));
    enrollment
        .activate(
            &EnrollmentActivation {
                request,
                nonce: challenge.nonce,
                signature: signature.to_bytes().to_vec(),
            },
            &format!("administration-purge-{label}-receipt-{suffix}"),
            &signing_key_id,
            now,
        )
        .await
        .expect("the repository enrollment should activate");
    SeedSigner::new(signing_key_id, node_id, &seed)
}

async fn insert_telemetry_event(
    database_url: &str,
    tenant_id: &str,
    repository_id: &str,
    telemetry_id: &str,
    occurred_at: SystemTime,
) {
    let (client, connection) = tokio_postgres::connect(database_url, tokio_postgres::NoTls)
        .await
        .expect("the test database should accept a direct fixture connection");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .execute(
            "INSERT INTO telemetry_events (tenant_id, repository_id, telemetry_id, node_id, \
                 kind, name, outcome, duration_ms, occurred_at) \
             VALUES ($1,$2,$3,'node',1,'name',1,0,$4)",
            &[&tenant_id, &repository_id, &telemetry_id, &occurred_at],
        )
        .await
        .expect("inserting a telemetry event fixture should succeed");
}

async fn administration_router(database_url: &str, tenant_id: &str) -> axum::Router {
    let fleet = Arc::new(
        FleetStore::connect(database_url)
            .await
            .expect("the test database should accept Fleet connections"),
    );
    let administration = AdministrationStore::connect(database_url)
        .await
        .expect("the test database should accept Administration store connections");
    let claims = ClaimStore::connect(database_url)
        .await
        .expect("the test database should accept Claim connections");
    administration_routes(AdministrationApiState::with_claims(
        fleet,
        Arc::from(tenant_id.to_owned()),
        Arc::new(Mutex::new(administration)),
        Arc::new(Mutex::new(claims)),
        None,
        None,
    ))
}

async fn post_json(router: &axum::Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .expect("the request should build"),
        )
        .await
        .expect("the request should receive a response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the response body should be bounded");
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("a non-empty response body should be JSON")
    };
    (status, json)
}

async fn get_json(router: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("the request should build"),
        )
        .await
        .expect("the request should receive a response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("the response body should be bounded");
    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("a non-empty response body should be JSON")
    };
    (status, json)
}

fn authentication_json(authentication: ackplane_protocol::v1::ClaimAuthentication) -> Value {
    json!({
        "signing_key_id": authentication.signing_key_id,
        "node_id": authentication.node_id,
        "signed_at": authentication.signed_at,
        "nonce": authentication.nonce,
        "signature": authentication.signature,
    })
}

fn preview_body(
    signer: &SeedSigner,
    tenant_id: &str,
    repository_id: &str,
    policy_id: &str,
    older_than_seconds: u64,
    idempotency_key: String,
) -> Value {
    let authentication = authenticate_lifecycle_purge(
        signer,
        tenant_id,
        repository_id,
        &LifecyclePurgeOperation::Preview {
            policy_id,
            data_category: DATA_CATEGORY,
            older_than_seconds,
            confirmation_window_seconds: CONFIRMATION_WINDOW_SECONDS,
            idempotency_key: &idempotency_key,
        },
    );
    json!({
        "policy_id": policy_id,
        "data_category": DATA_CATEGORY,
        "older_than_seconds": older_than_seconds,
        "confirmation_window_seconds": CONFIRMATION_WINDOW_SECONDS,
        "idempotency_key": idempotency_key,
        "authentication": authentication_json(authentication),
    })
}

fn confirmation_body(
    signer: &SeedSigner,
    tenant_id: &str,
    repository_id: &str,
    request_id: &str,
) -> Value {
    let authentication = authenticate_lifecycle_purge(
        signer,
        tenant_id,
        repository_id,
        &LifecyclePurgeOperation::Confirm { request_id },
    );
    json!({ "authentication": authentication_json(authentication) })
}

async fn adopt_purge_policy(router: &axum::Router, tenant_id: &str, suffix: &str) -> String {
    let (status, body) = post_json(
        router,
        "/api/v1/administration/policies",
        json!({
            "operation": "lifecycle_purge",
            "scope": "tenant",
            "tenant_id": tenant_id,
            "data_classification": "diagnostic-telemetry",
            "retention_basis": "operator-defined retention window, ADR-0134",
            "idempotency_key": format!("purge-policy-{suffix}"),
            "lifetime_seconds": 3600,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body was {body:?}");
    body["policy_id"]
        .as_str()
        .expect("the response should carry a policy_id")
        .to_string()
}

#[tokio::test]
async fn distinct_enrolled_keys_preview_and_confirm_a_scoped_purge() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let suffix = unique_suffix();
    let tenant_id = format!("tenant-administration-purge-{suffix}");
    let repository_id = format!("repository-administration-purge-{suffix}");
    let requester = enroll_signer(
        &database_url,
        &tenant_id,
        &repository_id,
        &suffix,
        "requester",
        [17; 32],
    )
    .await;
    let confirmer = enroll_signer(
        &database_url,
        &tenant_id,
        &repository_id,
        &suffix,
        "confirmer",
        [18; 32],
    )
    .await;
    let now = SystemTime::now();
    let cutoff = now - Duration::from_secs(3600);
    insert_telemetry_event(
        &database_url,
        &tenant_id,
        &repository_id,
        &format!("old-{suffix}"),
        cutoff - Duration::from_secs(60),
    )
    .await;
    insert_telemetry_event(
        &database_url,
        &tenant_id,
        &repository_id,
        &format!("new-{suffix}"),
        now,
    )
    .await;
    let router = administration_router(&database_url, &tenant_id).await;
    let policy_id = adopt_purge_policy(&router, &tenant_id, &suffix).await;
    let older_than_seconds = cutoff
        .duration_since(UNIX_EPOCH)
        .expect("cutoff is after the epoch")
        .as_secs();

    let (preview_status, preview_body) = post_json(
        &router,
        &format!("/api/v1/repositories/{repository_id}/administration/purges"),
        preview_body(
            &requester,
            &tenant_id,
            &repository_id,
            &policy_id,
            older_than_seconds,
            format!("purge-request-{suffix}"),
        ),
    )
    .await;
    assert_eq!(preview_status, StatusCode::OK, "body was {preview_body:?}");
    assert_eq!(preview_body["preview_row_count"], json!(1));
    let request_id = preview_body["request_id"]
        .as_str()
        .expect("the response should carry a request_id")
        .to_string();

    let (confirm_status, confirm_body) = post_json(
        &router,
        &format!("/api/v1/repositories/{repository_id}/administration/purges/{request_id}/confirm"),
        confirmation_body(&confirmer, &tenant_id, &repository_id, &request_id),
    )
    .await;
    assert_eq!(confirm_status, StatusCode::OK, "body was {confirm_body:?}");
    assert_eq!(confirm_body["outcome"], json!("succeeded"));
    assert_eq!(confirm_body["rows_deleted"], json!(1));
    assert_eq!(
        confirm_body["confirming_signing_key_id"],
        json!(confirmer.signing_key_id())
    );
    assert_eq!(
        confirm_body["confirming_node_id"],
        json!(confirmer.node_id())
    );

    let (status, body) = get_json(
        &router,
        &format!("/api/v1/repositories/{repository_id}/administration/purges/{request_id}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "body was {body:?}");
    assert_eq!(body["receipt_id"], confirm_body["receipt_id"]);
}

#[tokio::test]
async fn requester_key_cannot_confirm_its_own_preview_but_a_second_key_can() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let suffix = unique_suffix();
    let tenant_id = format!("tenant-administration-purge-self-confirm-{suffix}");
    let repository_id = format!("repository-administration-purge-self-confirm-{suffix}");
    let requester = enroll_signer(
        &database_url,
        &tenant_id,
        &repository_id,
        &suffix,
        "requester",
        [19; 32],
    )
    .await;
    let confirmer = enroll_signer(
        &database_url,
        &tenant_id,
        &repository_id,
        &suffix,
        "confirmer",
        [20; 32],
    )
    .await;
    let now = SystemTime::now();
    let cutoff = now - Duration::from_secs(3600);
    insert_telemetry_event(
        &database_url,
        &tenant_id,
        &repository_id,
        &format!("old-{suffix}"),
        cutoff - Duration::from_secs(60),
    )
    .await;
    let router = administration_router(&database_url, &tenant_id).await;
    let policy_id = adopt_purge_policy(&router, &tenant_id, &suffix).await;
    let request_id = {
        let (status, body) = post_json(
            &router,
            &format!("/api/v1/repositories/{repository_id}/administration/purges"),
            preview_body(
                &requester,
                &tenant_id,
                &repository_id,
                &policy_id,
                cutoff.duration_since(UNIX_EPOCH).unwrap().as_secs(),
                format!("purge-request-{suffix}"),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body was {body:?}");
        body["request_id"].as_str().unwrap().to_string()
    };

    let (self_status, self_body) = post_json(
        &router,
        &format!("/api/v1/repositories/{repository_id}/administration/purges/{request_id}/confirm"),
        confirmation_body(&requester, &tenant_id, &repository_id, &request_id),
    )
    .await;
    assert_eq!(self_status, StatusCode::FORBIDDEN, "body was {self_body:?}");
    let (before_retry_status, _) = get_json(
        &router,
        &format!("/api/v1/repositories/{repository_id}/administration/purges/{request_id}"),
    )
    .await;
    assert_eq!(before_retry_status, StatusCode::NOT_FOUND);

    let (retry_status, retry_body) = post_json(
        &router,
        &format!("/api/v1/repositories/{repository_id}/administration/purges/{request_id}/confirm"),
        confirmation_body(&confirmer, &tenant_id, &repository_id, &request_id),
    )
    .await;
    assert_eq!(retry_status, StatusCode::OK, "body was {retry_body:?}");
}

#[tokio::test]
async fn invalid_or_replayed_confirmation_authentication_cannot_execute_a_purge() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let suffix = unique_suffix();
    let tenant_id = format!("tenant-administration-purge-replay-{suffix}");
    let repository_id = format!("repository-administration-purge-replay-{suffix}");
    let requester = enroll_signer(
        &database_url,
        &tenant_id,
        &repository_id,
        &suffix,
        "requester",
        [21; 32],
    )
    .await;
    let confirmer = enroll_signer(
        &database_url,
        &tenant_id,
        &repository_id,
        &suffix,
        "confirmer",
        [22; 32],
    )
    .await;
    let now = SystemTime::now();
    let cutoff = now - Duration::from_secs(3600);
    insert_telemetry_event(
        &database_url,
        &tenant_id,
        &repository_id,
        &format!("old-{suffix}"),
        cutoff - Duration::from_secs(60),
    )
    .await;
    let router = administration_router(&database_url, &tenant_id).await;
    let policy_id = adopt_purge_policy(&router, &tenant_id, &suffix).await;
    let request_id = {
        let (status, body) = post_json(
            &router,
            &format!("/api/v1/repositories/{repository_id}/administration/purges"),
            preview_body(
                &requester,
                &tenant_id,
                &repository_id,
                &policy_id,
                cutoff.duration_since(UNIX_EPOCH).unwrap().as_secs(),
                format!("purge-request-{suffix}"),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "body was {body:?}");
        body["request_id"].as_str().unwrap().to_string()
    };

    let mut invalid_body = confirmation_body(&confirmer, &tenant_id, &repository_id, &request_id);
    invalid_body["authentication"]["signature"] = Value::Array(vec![Value::from(0); 64]);
    let (invalid_status, _) = post_json(
        &router,
        &format!("/api/v1/repositories/{repository_id}/administration/purges/{request_id}/confirm"),
        invalid_body,
    )
    .await;
    assert_eq!(invalid_status, StatusCode::UNAUTHORIZED);

    let replay_body = confirmation_body(&confirmer, &tenant_id, &repository_id, &request_id);
    let (first_status, _) = post_json(
        &router,
        &format!("/api/v1/repositories/{repository_id}/administration/purges/{request_id}/confirm"),
        replay_body.clone(),
    )
    .await;
    assert_eq!(first_status, StatusCode::OK);
    let (replay_status, _) = post_json(
        &router,
        &format!("/api/v1/repositories/{repository_id}/administration/purges/{request_id}/confirm"),
        replay_body,
    )
    .await;
    assert_eq!(replay_status, StatusCode::UNAUTHORIZED);
}

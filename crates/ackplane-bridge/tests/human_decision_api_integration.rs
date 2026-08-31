//! Real PostgreSQL-gated coverage for Bridge's human decision queue
//! (ADR-0115 item 5: an escalation appears in a human queue rather than
//! staying hidden in agent logs).

use std::{sync::Arc, time::Duration, time::SystemTime};

use ackplane_bridge::human_decision_api::{human_decision_routes, HumanDecisionApiState};
use ackplane_server::{
    enrollment::{activation_challenge_bytes, public_key_fingerprint},
    enrollment_store::{
        ActivationChallengeRequest, EnrollmentActivation, EnrollmentApproval, EnrollmentStore,
        EnrollmentSubmission,
    },
    fleet::FleetStore,
    human_decision_store::{
        HumanDecisionRequest, HumanDecisionResolutionOutcome, HumanDecisionResolutionRequest,
        HumanDecisionStore, SafeBehavior,
    },
};
use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use ed25519_dalek::{Signer, SigningKey};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

fn unique_id(prefix: &str) -> String {
    let mut bytes = [0_u8; 8];
    getrandom::getrandom(&mut bytes).expect("the OS random source should be available");
    let suffix: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{prefix}-{suffix}")
}

async fn enroll_repository(database_url: &str, tenant_id: &str, repository_id: &str, unique: &str) {
    let seed: [u8; 32] = Sha256::digest(format!("key-{unique}").as_bytes()).into();
    let signing_key = SigningKey::from_bytes(&seed);
    let public_key = signing_key.verifying_key().to_bytes();
    let request_id = format!("request-{unique}");
    let node_id = format!("node-{unique}");
    let fingerprint = public_key_fingerprint(&public_key);
    let submission = EnrollmentSubmission {
        request_id: request_id.clone(),
        tenant_id: tenant_id.to_string(),
        repository_id: repository_id.to_string(),
        proposed_node_id: node_id.clone(),
        display_name: "Human decision API integration node".to_string(),
        public_key: public_key.to_vec(),
        public_key_fingerprint: fingerprint.clone(),
        requested_capabilities: vec!["synchronize".to_string()],
        created_at: "2026-01-01T00:00:00Z".to_string(),
        expires_at: "2030-01-01T00:00:00Z".to_string(),
    };
    let request = ActivationChallengeRequest {
        request_id,
        tenant_id: tenant_id.to_string(),
        repository_id: repository_id.to_string(),
        proposed_node_id: node_id,
        public_key_fingerprint: fingerprint,
    };
    let enrollment_pool = ackplane_server::db_pool::build_pool(
        database_url,
        ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
    )
    .expect("the test pool builds from a valid database url");
    let enrollment = EnrollmentStore::connect(&enrollment_pool)
        .await
        .expect("connect enrollment store");
    enrollment
        .submit(&submission)
        .await
        .expect("submit enrollment");
    enrollment
        .approve(&EnrollmentApproval {
            request_id: submission.request_id.clone(),
            tenant_id: tenant_id.to_string(),
            repository_id: repository_id.to_string(),
            public_key_fingerprint: submission.public_key_fingerprint.clone(),
            approved_capabilities: submission.requested_capabilities.clone(),
            approved_by: "human-decision-api-integration-administrator".to_string(),
        })
        .await
        .expect("approve enrollment");
    let nonce: [u8; 32] = Sha256::digest(format!("nonce-{unique}").as_bytes()).into();
    let challenge = enrollment
        .issue_challenge(&request, &nonce, SystemTime::now())
        .await
        .expect("issue enrollment challenge");
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
            &format!("receipt-{unique}"),
            &format!("signing-key-{unique}"),
            SystemTime::now(),
        )
        .await
        .expect("activate enrollment");
}

fn decision_request(
    tenant_id: &str,
    repository_id: &str,
    label: &str,
    idempotency_key: &str,
) -> HumanDecisionRequest {
    HumanDecisionRequest {
        tenant_id: tenant_id.to_string(),
        repository_id: repository_id.to_string(),
        verified_proposing_principal_id: format!("principal:agent-{label}"),
        proposed_action: format!("action:{label}"),
        target: format!("artifact:{label}"),
        reason: format!("outside the delegation envelope: {label}"),
        context_packet_digest: vec![0xab; 32],
        evidence_digest: vec![0xcd; 32],
        alternatives: "narrow the scope or wait for a broader delegation".to_string(),
        safe_behavior: SafeBehavior::CheckpointAndPause,
        related_delegation_id: Some(format!("delegation-{label}")),
        expires_at: SystemTime::now() + Duration::from_secs(3_600),
        idempotency_key: idempotency_key.to_string(),
    }
}

async fn application(database_url: &str, tenant_id: &str) -> axum::Router {
    let db_pool = ackplane_server::db_pool::build_pool(
        database_url,
        ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
    )
    .expect("the gated test database url builds a pool");
    let decisions = Arc::new(
        HumanDecisionStore::connect(&db_pool)
            .await
            .expect("connect Human decision store"),
    );
    let fleet = Arc::new(
        FleetStore::connect(database_url)
            .await
            .expect("connect Fleet store"),
    );
    human_decision_routes(HumanDecisionApiState::new(
        decisions,
        fleet,
        Arc::from(tenant_id.to_string()),
    ))
}

async fn body_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), 1_048_576)
        .await
        .expect("read Bridge response body");
    serde_json::from_slice(&body).expect("parse Bridge JSON response")
}

async fn get(app: &axum::Router, uri: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("build Bridge request"),
        )
        .await
        .expect("serve Bridge route")
}

#[tokio::test]
async fn the_decision_queue_shows_waiting_escalations_and_their_resolutions() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let unique = unique_id("human-decision-api");
    let tenant_id = format!("tenant-{unique}");
    let repository_id = format!("repository-{unique}");
    enroll_repository(&database_url, &tenant_id, &repository_id, &unique).await;
    let fixture_pool = ackplane_server::db_pool::build_pool(
        &database_url,
        ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
    )
    .expect("the gated test database url builds a pool");
    let store = HumanDecisionStore::connect(&fixture_pool)
        .await
        .expect("connect Human decision store for fixtures");
    let waiting = store
        .request(decision_request(
            &tenant_id,
            &repository_id,
            "export",
            &format!("{unique}-request-export"),
        ))
        .await
        .expect("record waiting escalation");
    let resolved = store
        .request(decision_request(
            &tenant_id,
            &repository_id,
            "widen-scope",
            &format!("{unique}-request-widen"),
        ))
        .await
        .expect("record escalation to resolve");
    store
        .resolve(HumanDecisionResolutionRequest {
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            decision_id: resolved.projection.decision_id.clone(),
            verified_resolving_principal_id: "principal:human-reviewer".to_string(),
            outcome: HumanDecisionResolutionOutcome::Denied,
            rationale: "the requested scope exceeds the approved envelope".to_string(),
            expected_version: resolved.projection.version,
            idempotency_key: format!("{unique}-resolve-widen"),
        })
        .await
        .expect("resolve escalation");
    let app = application(&database_url, &tenant_id).await;

    let page = get(&app, "/decisions").await;
    assert_eq!(page.status(), StatusCode::OK);

    let all = get(
        &app,
        &format!("/api/v1/repositories/{repository_id}/decisions"),
    )
    .await;
    assert_eq!(all.status(), StatusCode::OK);
    let all = body_json(all).await;
    let entries = all["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 2, "both escalations stay listable");

    // ADR-0115 item 5: the queue must carry what a human needs to decide, not
    // just an identifier they have to go and look up somewhere else.
    let waiting_entry = entries
        .iter()
        .find(|entry| entry["decision_id"] == waiting.projection.decision_id.as_str())
        .expect("the waiting escalation is in the queue");
    assert_eq!(waiting_entry["state"], "pending");
    assert_eq!(waiting_entry["proposed_action"], "action:export");
    assert_eq!(waiting_entry["target"], "artifact:export");
    assert_eq!(
        waiting_entry["reason"],
        "outside the delegation envelope: export"
    );
    assert_eq!(
        waiting_entry["alternatives"],
        "narrow the scope or wait for a broader delegation"
    );
    assert_eq!(waiting_entry["safe_behavior"], "checkpoint_and_pause");
    assert_eq!(waiting_entry["context_packet_digest"], "ab".repeat(32));
    assert_eq!(waiting_entry["evidence_digest"], "cd".repeat(32));
    assert!(waiting_entry["expires_at_seconds"].is_number());

    // A resolution keeps the principal and rationale that justified it
    // (ADR-0115 item 8), so review can see who decided and why.
    let resolved_entry = entries
        .iter()
        .find(|entry| entry["decision_id"] == resolved.projection.decision_id.as_str())
        .expect("the resolved escalation stays visible");
    assert_eq!(resolved_entry["state"], "denied");
    assert_eq!(
        resolved_entry["resolved_by_principal_id"],
        "principal:human-reviewer"
    );
    assert_eq!(
        resolved_entry["resolution_rationale"],
        "the requested scope exceeds the approved envelope"
    );

    let pending_only = get(
        &app,
        &format!("/api/v1/repositories/{repository_id}/decisions?status=pending"),
    )
    .await;
    assert_eq!(pending_only.status(), StatusCode::OK);
    let pending_only = body_json(pending_only).await;
    let pending_entries = pending_only["entries"].as_array().expect("entries array");
    assert_eq!(pending_entries.len(), 1, "only the waiting escalation");
    assert_eq!(
        pending_entries[0]["decision_id"],
        waiting.projection.decision_id.as_str()
    );

    let single = get(
        &app,
        &format!(
            "/api/v1/repositories/{repository_id}/decisions/{}",
            waiting.projection.decision_id
        ),
    )
    .await;
    assert_eq!(single.status(), StatusCode::OK);
    assert_eq!(
        body_json(single).await["decision_id"],
        waiting.projection.decision_id.as_str()
    );

    let missing = get(
        &app,
        &format!("/api/v1/repositories/{repository_id}/decisions/decision-does-not-exist"),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);

    let unknown_status = get(
        &app,
        &format!("/api/v1/repositories/{repository_id}/decisions?status=maybe"),
    )
    .await;
    assert_eq!(
        unknown_status.status(),
        StatusCode::BAD_REQUEST,
        "an unrecognised status filter is refused rather than silently ignored"
    );
}

/// ADR-0098 decision 5: every Bridge read is scoped by repository visibility,
/// so a caller cannot read one tenant's escalations through another tenant's
/// Bridge just by knowing the repository id.
#[tokio::test]
async fn the_decision_queue_refuses_a_repository_outside_the_bridge_tenant() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let unique = unique_id("human-decision-api-scope");
    let tenant_id = format!("tenant-{unique}");
    let repository_id = format!("repository-{unique}");
    enroll_repository(&database_url, &tenant_id, &repository_id, &unique).await;
    let fixture_pool = ackplane_server::db_pool::build_pool(
        &database_url,
        ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
    )
    .expect("the gated test database url builds a pool");
    let store = HumanDecisionStore::connect(&fixture_pool)
        .await
        .expect("connect Human decision store for fixtures");
    store
        .request(decision_request(
            &tenant_id,
            &repository_id,
            "export",
            &format!("{unique}-request-export"),
        ))
        .await
        .expect("record waiting escalation");

    let other_tenant_app = application(&database_url, &format!("tenant-other-{unique}")).await;
    let refused = get(
        &other_tenant_app,
        &format!("/api/v1/repositories/{repository_id}/decisions"),
    )
    .await;
    assert_eq!(refused.status(), StatusCode::NOT_FOUND);

    let same_tenant_app = application(&database_url, &tenant_id).await;
    let unknown_repository = get(
        &same_tenant_app,
        "/api/v1/repositories/repository-never-enrolled/decisions",
    )
    .await;
    assert_eq!(unknown_repository.status(), StatusCode::NOT_FOUND);
}

/// A page boundary must not drop or duplicate a waiting escalation: the
/// cursor addresses a stable durable position, not an offset into a result
/// set that shifts as new requests arrive.
#[tokio::test]
async fn the_decision_queue_pages_without_dropping_or_repeating_a_request() {
    let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
        println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
        return;
    };
    let unique = unique_id("human-decision-api-page");
    let tenant_id = format!("tenant-{unique}");
    let repository_id = format!("repository-{unique}");
    enroll_repository(&database_url, &tenant_id, &repository_id, &unique).await;
    let fixture_pool = ackplane_server::db_pool::build_pool(
        &database_url,
        ackplane_server::db_pool::TEST_POOL_MAX_SIZE,
    )
    .expect("the gated test database url builds a pool");
    let store = HumanDecisionStore::connect(&fixture_pool)
        .await
        .expect("connect Human decision store for fixtures");
    let mut expected = Vec::new();
    for index in 0..3 {
        let outcome = store
            .request(decision_request(
                &tenant_id,
                &repository_id,
                &format!("escalation-{index}"),
                &format!("{unique}-request-{index}"),
            ))
            .await
            .expect("record escalation");
        expected.push(outcome.projection.decision_id);
    }
    let app = application(&database_url, &tenant_id).await;

    let first = body_json(
        get(
            &app,
            &format!("/api/v1/repositories/{repository_id}/decisions?limit=2"),
        )
        .await,
    )
    .await;
    let first_ids: Vec<String> = first["entries"]
        .as_array()
        .expect("entries array")
        .iter()
        .map(|entry| entry["decision_id"].as_str().expect("id").to_string())
        .collect();
    assert_eq!(first_ids.len(), 2);
    let cursor = &first["next_after"];
    assert!(!cursor.is_null(), "a further page is reported");

    let second = body_json(
        get(
            &app,
            &format!(
                "/api/v1/repositories/{repository_id}/decisions?limit=2\
                 &after_source_event_position={}&after_decision_id={}",
                cursor["source_event_position"],
                cursor["decision_id"].as_str().expect("cursor id")
            ),
        )
        .await,
    )
    .await;
    let second_ids: Vec<String> = second["entries"]
        .as_array()
        .expect("entries array")
        .iter()
        .map(|entry| entry["decision_id"].as_str().expect("id").to_string())
        .collect();
    assert_eq!(second_ids.len(), 1);
    assert!(
        second_ids.iter().all(|id| !first_ids.contains(id)),
        "the second page must not repeat the first"
    );
    let mut seen = first_ids;
    seen.extend(second_ids);
    seen.sort();
    expected.sort();
    assert_eq!(seen, expected, "every escalation appears exactly once");
    assert!(
        second["next_after"].is_null(),
        "the last page ends the walk"
    );
}

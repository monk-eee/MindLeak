use std::time::SystemTime;

use ackplane_protocol::enrollment::{activation_challenge_bytes, public_key_fingerprint};
use ackplane_server::enrollment_store::{
    ActivationChallengeRequest, EnrollmentActivation, EnrollmentApproval, EnrollmentStore,
    EnrollmentSubmission,
};
use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};

pub async fn enroll_repository(
    database_url: &str,
    tenant_id: &str,
    repository_id: &str,
    unique: &str,
) {
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
        display_name: "Design API integration node".to_string(),
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
            approved_by: "design-api-integration-administrator".to_string(),
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

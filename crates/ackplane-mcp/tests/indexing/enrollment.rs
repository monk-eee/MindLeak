use ackplane_node::SigningBinding;
use ackplane_protocol::{
    enrollment::{activation_challenge_bytes, public_key_fingerprint},
    v1::{self, node_enrollment_service_client::NodeEnrollmentServiceClient},
};
use ackplane_server::{
    db_pool::PgPool,
    enrollment_store::{EnrollmentApproval, EnrollmentStore},
};
use ed25519_dalek::{Signer, SigningKey};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tonic::Request;

pub const SEED: [u8; 32] = [113; 32];

pub async fn activate(
    pool: &PgPool,
    endpoint: &str,
    tenant_id: &str,
    repository_id: &str,
    seed: &[u8; 32],
) -> SigningBinding {
    let key = SigningKey::from_bytes(seed);
    let public_key = key.verifying_key().to_bytes().to_vec();
    let fingerprint = public_key_fingerprint(&public_key);
    let request_id = format!("{repository_id}-{fingerprint}-request");
    let node_id = format!("{repository_id}-{fingerprint}-node");
    let capabilities = vec!["synchronize".to_string(), "mcp-front-door".to_string()];
    let now = OffsetDateTime::now_utc();
    let mut client = NodeEnrollmentServiceClient::connect(endpoint.to_string())
        .await
        .unwrap();
    client
        .submit_enrollment_request(Request::new(v1::EnrollmentRequest {
            request_id: request_id.clone(),
            tenant_id: tenant_id.into(),
            repository_id: repository_id.into(),
            proposed_node_id: node_id.clone(),
            display_name: "indexing end-to-end test node".into(),
            public_key_fingerprint: fingerprint.clone(),
            requested_capabilities: capabilities.clone(),
            created_at: now.format(&Rfc3339).unwrap(),
            expires_at: (now + time::Duration::hours(1)).format(&Rfc3339).unwrap(),
            public_key,
        }))
        .await
        .expect("submit a real enrollment request");
    EnrollmentStore::connect(pool)
        .await
        .unwrap()
        .approve(&EnrollmentApproval {
            request_id: request_id.clone(),
            tenant_id: tenant_id.into(),
            repository_id: repository_id.into(),
            public_key_fingerprint: fingerprint.clone(),
            approved_capabilities: capabilities,
            approved_by: "index-test-administrator".into(),
        })
        .await
        .expect("approve the pending test enrollment");
    let challenge = client
        .get_activation_challenge(Request::new(v1::EnrollmentChallengeRequest {
            request_id: request_id.clone(),
            tenant_id: tenant_id.into(),
            repository_id: repository_id.into(),
            proposed_node_id: node_id.clone(),
            public_key_fingerprint: fingerprint.clone(),
        }))
        .await
        .expect("obtain the server's activation nonce")
        .into_inner();
    let proof = activation_challenge_bytes(
        &challenge.nonce,
        &request_id,
        tenant_id,
        repository_id,
        &node_id,
        &fingerprint,
    );
    let activation = client
        .activate_enrollment(Request::new(v1::EnrollmentActivationProof {
            request_id,
            tenant_id: tenant_id.into(),
            repository_id: repository_id.into(),
            proposed_node_id: node_id.clone(),
            public_key_fingerprint: fingerprint,
            nonce: challenge.nonce,
            signature: key.sign(&proof).to_bytes().to_vec(),
        }))
        .await
        .expect("activate using a real proof of possession")
        .into_inner();
    assert!(!activation.signing_key_id.is_empty());
    SigningBinding {
        tenant_id: tenant_id.into(),
        repository_id: repository_id.into(),
        node_id,
        key_id: activation.signing_key_id,
    }
}

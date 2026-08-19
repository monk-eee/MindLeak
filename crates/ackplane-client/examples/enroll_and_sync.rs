//! Enroll one real node with a running `ackplane-server` over genuine gRPC,
//! then publish one real signed structural fact, so the Bridge Fleet view has
//! a real, projected repository to show (ADR-0085, ADR-0083, ADR-0084,
//! ADR-0086 clause 9, ADR-0087).
//!
//! An administrator must approve every enrollment request (ADR-0085: a node
//! is never allowed to approve itself, and the wire contract deliberately has
//! no RPC for it). This example plays both parts for demonstration purposes:
//! it submits the request as a node would, then approves it the only way an
//! administrator can today -- direct database access via `EnrollmentStore`,
//! the same dev-dependency this crate's own `tests/arbitration.rs` already
//! uses for equivalent fixture setup. Everything else -- submit, the
//! activation challenge/response, and the Synchronize handshake and event --
//! is genuine gRPC against the real service, exactly as an external
//! repository would drive it.
//!
//! Run against the compose topology with, e.g.:
//! ```text
//! docker compose up -d postgres
//! $env:ACKPLANE_DATABASE_URL = "postgresql://ackplane:ackplane-development-only-not-for-production@127.0.0.1:5432/ackplane"
//! $env:ACKPLANE_GRPC_ENDPOINT = "http://127.0.0.1:8443"
//! cargo run --example enroll_and_sync -p ackplane-client
//! ```
//!
//! To make the enrolled repository visible in a locally running Bridge's
//! Fleet view (ADR-0095), also set `ACKPLANE_BRIDGE_SALT_PATH` and
//! `ACKPLANE_BRIDGE_DEVELOPMENT_TENANT` to the exact same values the Bridge
//! itself was started with -- this example then derives `tenant_id` the same
//! way the Bridge's loopback developer profile does (ADR-0098 decision 3:
//! `hex(SHA-256(salt || tenant_name))`), so the two agree on which tenant's
//! data to show. Without them, it falls back to a literal tenant id, useful
//! for exercising the protocol without a Bridge in the loop at all.

use ed25519_dalek::{Signer, SigningKey};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Request;

use ackplane_protocol::v1::{
    self, node_enrollment_service_client::NodeEnrollmentServiceClient,
    node_sync_service_client::NodeSyncServiceClient,
};
use ackplane_server::enrollment::{
    activation_challenge_bytes, connection_challenge_bytes, public_key_fingerprint,
    ConnectionChallengeBinding,
};
use ackplane_server::enrollment_store::{EnrollmentApproval, EnrollmentStore};
use ackplane_server::envelope_signature::envelope_signing_bytes;
use ackplane_server::projection::{StructuralFact, STRUCTURAL_FACT_PAYLOAD_TYPE};

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("the current instant should always format as RFC 3339")
}

fn unique_request_id() -> Result<String, Box<dyn std::error::Error>> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    Ok(format!("request-enroll-and-sync-example-{nanos}"))
}

/// The exact token a locally running Bridge derives for its loopback
/// developer profile (ADR-0098 decision 3): `hex(SHA-256(salt || tenant_name))`.
fn development_tenant_token(salt: &[u8], tenant_name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(tenant_name.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Resolve the tenant id to enroll under: the Bridge's derived developer
/// token when both `ACKPLANE_BRIDGE_SALT_PATH` and
/// `ACKPLANE_BRIDGE_DEVELOPMENT_TENANT` are set, otherwise a literal id.
fn resolve_tenant_id() -> Result<String, Box<dyn std::error::Error>> {
    match (
        std::env::var("ACKPLANE_BRIDGE_SALT_PATH"),
        std::env::var("ACKPLANE_BRIDGE_DEVELOPMENT_TENANT"),
    ) {
        (Ok(salt_path), Ok(tenant_name)) => {
            let salt = std::fs::read(&salt_path).map_err(|error| {
                format!("could not read ACKPLANE_BRIDGE_SALT_PATH ({salt_path}): {error}")
            })?;
            Ok(development_tenant_token(&salt, &tenant_name))
        }
        _ => {
            Ok(std::env::var("ACKPLANE_EXAMPLE_TENANT_ID")
                .unwrap_or_else(|_| "example-tenant".into()))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db_url = std::env::var("ACKPLANE_DATABASE_URL")
        .map_err(|_| "set ACKPLANE_DATABASE_URL to the Ackplane Postgres connection string")?;
    let grpc_endpoint =
        std::env::var("ACKPLANE_GRPC_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:8443".into());
    let tenant_id = resolve_tenant_id()?;
    let repository_id =
        std::env::var("ACKPLANE_EXAMPLE_REPOSITORY_ID").unwrap_or_else(|_| "example-repo".into());
    let node_id =
        std::env::var("ACKPLANE_EXAMPLE_NODE_ID").unwrap_or_else(|_| "example-node".into());
    let request_id = unique_request_id()?;

    // A repository-owned, non-exporting signer in production (ADR-0100); a
    // fixed test seed here since this example's only job is to prove the
    // wire protocol, not to demonstrate key custody.
    let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
    let public_key = signing_key.verifying_key().to_bytes().to_vec();
    let fingerprint = public_key_fingerprint(&public_key);

    println!("tenant_id = {tenant_id}, repository_id = {repository_id}, node_id = {node_id}");
    println!("public_key_fingerprint = {fingerprint}");

    let mut enrollment_client = NodeEnrollmentServiceClient::connect(grpc_endpoint.clone()).await?;

    // 1. Submit the enrollment request over real gRPC, as an external node would.
    let submit_status = enrollment_client
        .submit_enrollment_request(Request::new(v1::EnrollmentRequest {
            request_id: request_id.clone(),
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            proposed_node_id: node_id.clone(),
            display_name: "enroll_and_sync example node".to_string(),
            public_key_fingerprint: fingerprint.clone(),
            requested_capabilities: vec!["synchronize".to_string()],
            created_at: now_rfc3339(),
            expires_at: "2030-01-01T00:00:00Z".to_string(),
            public_key: public_key.clone(),
        }))
        .await?
        .into_inner();
    println!("submit_enrollment_request -> {submit_status:?}");

    // 2. Approve it. Standing in for the administrator ADR-0085 requires,
    // played here for demonstration only -- see the module doc comment.
    let mut store = EnrollmentStore::connect(&db_url).await?;
    let approval = store
        .approve(&EnrollmentApproval {
            request_id: request_id.clone(),
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            public_key_fingerprint: fingerprint.clone(),
            approved_capabilities: vec!["synchronize".to_string()],
            approved_by: "enroll_and_sync-example-administrator".to_string(),
        })
        .await?;
    println!("approve -> {approval:?}");

    // 3. Real gRPC activation challenge and signed proof of possession.
    let challenge = enrollment_client
        .get_activation_challenge(Request::new(v1::EnrollmentChallengeRequest {
            request_id: request_id.clone(),
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            proposed_node_id: node_id.clone(),
            public_key_fingerprint: fingerprint.clone(),
        }))
        .await?
        .into_inner();
    let proof_bytes = activation_challenge_bytes(
        &challenge.nonce,
        &request_id,
        &tenant_id,
        &repository_id,
        &node_id,
        &fingerprint,
    );
    let signature = signing_key.sign(&proof_bytes).to_bytes().to_vec();
    let activation = enrollment_client
        .activate_enrollment(Request::new(v1::EnrollmentActivationProof {
            request_id: request_id.clone(),
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            proposed_node_id: node_id.clone(),
            public_key_fingerprint: fingerprint.clone(),
            nonce: challenge.nonce.clone(),
            signature,
        }))
        .await?
        .into_inner();
    println!("activate_enrollment -> {activation:?}");

    // The wire contract does not return the server-assigned signing_key_id to
    // the node that just activated (see
    // gaps.d/enrolment-activation-never-returns-the-assigned-signing-key-id.md);
    // read it back the only way available today: query the registry directly
    // by fingerprint, which only this example's administrator stand-in can do.
    let (db_client, connection) = tokio_postgres::connect(&db_url, tokio_postgres::NoTls).await?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let row = db_client
        .query_one(
            "SELECT signing_key_id FROM signing_keys WHERE public_key_fingerprint = $1 \
             ORDER BY activated_at DESC LIMIT 1",
            &[&fingerprint],
        )
        .await?;
    let signing_key_id: String = row.get(0);
    println!("assigned signing_key_id = {signing_key_id}");

    // 4. The real NodeSync stream: Hello -> ConnectionChallenge ->
    // ChallengeResponse -> HelloAccepted -> FlowControl, then one signed event.
    let mut sync_client = NodeSyncServiceClient::connect(grpc_endpoint).await?;
    let (tx, rx) = mpsc::channel::<v1::NodeFrame>(4);
    let response = sync_client
        .synchronize(Request::new(ReceiverStream::new(rx)))
        .await?;
    let mut inbound = response.into_inner();

    tx.send(v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::Hello(v1::Hello {
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            producer_id: node_id.clone(),
            last_accepted_position: 0,
            capabilities: vec!["synchronize".to_string()],
            signing_key_id: signing_key_id.clone(),
        })),
    })
    .await?;

    let challenge_frame = inbound
        .message()
        .await?
        .ok_or("stream closed before ConnectionChallenge")?;
    let nonce = match challenge_frame.frame {
        Some(v1::ackplane_frame::Frame::ConnectionChallenge(challenge)) => challenge.nonce,
        other => return Err(format!("expected ConnectionChallenge, got {other:?}").into()),
    };
    let connection_signature = signing_key
        .sign(&connection_challenge_bytes(&ConnectionChallengeBinding {
            nonce: &nonce,
            tenant_id: &tenant_id,
            repository_id: &repository_id,
            producer_id: &node_id,
            signing_key_id: &signing_key_id,
        }))
        .to_bytes()
        .to_vec();
    tx.send(v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::ChallengeResponse(
            v1::ChallengeResponse {
                signature: connection_signature,
            },
        )),
    })
    .await?;

    let accepted_frame = inbound
        .message()
        .await?
        .ok_or("stream closed before HelloAccepted")?;
    println!("hello_accepted -> {accepted_frame:?}");
    let flow_control_frame = inbound
        .message()
        .await?
        .ok_or("stream closed before FlowControl")?;
    println!("flow_control -> {flow_control_frame:?}");

    // 5. One real signed event carrying a genuine structural fact -- one node
    // representing this repository -- so the ledger's stream head actually
    // moves AND the projection worker (ADR-0086 clause 9) has something real
    // to fold into the Bridge Fleet view's graph projection. An opaque
    // payload would move the ledger position but the projector's
    // `payload_type` filter would ignore it forever.
    let fact = StructuralFact {
        node_id: format!("repository:{repository_id}"),
        node_type: "repository".to_string(),
        label: repository_id.clone(),
        edges: Vec::new(),
    };
    let payload = serde_json::to_vec(&fact)?;
    let payload_digest = Sha256::digest(&payload).to_vec();
    let mut wire = v1::EventEnvelope {
        tenant_id: tenant_id.clone(),
        repository_id: repository_id.clone(),
        producer_id: node_id.clone(),
        producer_sequence: 1,
        payload,
        payload_digest,
        schema_version: "1".to_string(),
        occurred_at: now_rfc3339(),
        payload_type: STRUCTURAL_FACT_PAYLOAD_TYPE.to_string(),
        previous_envelope_digest: Vec::new(),
        signing_key_id: signing_key_id.clone(),
        signature: Vec::new(),
        provenance: v1::ProvenanceClass::EnrolledNode as i32,
    };
    wire.signature = signing_key
        .sign(&envelope_signing_bytes(&wire))
        .to_bytes()
        .to_vec();

    tx.send(v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::EventBatch(v1::EventBatch {
            events: vec![wire],
        })),
    })
    .await?;

    let receipt_frame = inbound
        .message()
        .await?
        .ok_or("stream closed before BatchReceipt")?;
    println!("batch_receipt -> {receipt_frame:?}");

    drop(tx);
    println!("enroll_and_sync example complete");
    Ok(())
}

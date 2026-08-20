//! `register-me`: drive the real ADR-0085 enrollment ceremony from the
//! command line, as three explicit steps that mirror the real actors:
//!
//!   register-me request  --repo R --node N (--tenant-name T --salt-path PATH | --tenant-id ID)
//!   register-me approve  --request-id ID (--tenant-name T --salt-path PATH | --tenant-id ID)
//!                        --repo R --fingerprint FP --admin-database-url URL
//!   register-me activate --request-id ID --key-path PATH
//!
//! `request` is the only step a node ever runs unattended. `approve` is a
//! separate administrator action — ADR-0085 gives a node no way to approve
//! itself, and no gRPC endpoint or UI for it exists yet, so this step calls
//! the enrollment store directly. That is a documented, single-operator
//! developer shortcut standing in for the not-yet-built approval surface,
//! never a claim that this is how a real deployment's administrator works.
//! `activate` proves possession of the approved key, then opens one real
//! `NodeSync` stream and sends a single signed heartbeat event so the node
//! is visibly live (e.g. in the Bridge Fleet view). `EnrollmentActivationResult`
//! returns the assigned `signing_key_id` directly, so `activate` needs no
//! database access at all.
//!
//! `--tenant-name` + `--salt-path` derive the same tenant id the Bridge
//! queries for (ADR-0098 decision 3) -- use it, or an enrolled repository
//! will never appear there. `--tenant-id` is a raw override for a deployment
//! that assigns tenant ids some other way (e.g. real OIDC).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::ExitCode,
};

use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
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

const DEFAULT_GRPC_ENDPOINT: &str = "http://127.0.0.1:8443";

/// Enrollment state `request` saves and `activate` reads back, so the node
/// only has to type its request id a second time, never re-derive anything.
#[derive(Serialize, Deserialize)]
struct SavedRequest {
    request_id: String,
    tenant_id: String,
    repository_id: String,
    node_id: String,
    public_key_fingerprint: String,
    grpc_endpoint: String,
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("format now")
}

/// The same `hex(SHA-256(salt || tenant_name))` the Bridge derives (ADR-0098
/// decision 3). A node must enroll under this exact value, not the bare
/// tenant name, or the Bridge's Fleet query -- scoped to its own derived
/// token -- will never find it.
fn dev_tenant_token(salt: &[u8], tenant_name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(tenant_name.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Simple `--flag value` parser: no CLI-argument-parsing dependency exists
/// anywhere in this workspace yet, and this surface is small enough not to
/// be the reason to add one.
fn parse_flags(args: &[String]) -> HashMap<String, String> {
    let mut flags = HashMap::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(name) = arg.strip_prefix("--") {
            if let Some(value) = iter.next() {
                flags.insert(name.to_string(), value.clone());
            }
        }
    }
    flags
}

fn require<'a>(flags: &'a HashMap<String, String>, name: &str) -> Result<&'a str, String> {
    flags
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| format!("--{name} is required"))
}

fn key_path(flags: &HashMap<String, String>, node_id: &str) -> PathBuf {
    flags
        .get("key-path")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("register-me-{node_id}.key")))
}

fn state_path(key_path: &Path) -> PathBuf {
    let mut path = key_path.as_os_str().to_owned();
    path.push(".enrollment.json");
    PathBuf::from(path)
}

/// Resolve the wire `tenant_id`: derive it from `--tenant-name` + `--salt-path`
/// (matches what the Bridge will query for), or take `--tenant-id` directly
/// for a deployment that assigns tenant ids some other way (e.g. real OIDC).
fn resolve_tenant_id(flags: &HashMap<String, String>) -> Result<String, String> {
    if let Some(tenant_id) = flags.get("tenant-id") {
        return Ok(tenant_id.clone());
    }
    let tenant_name = require(flags, "tenant-name")
        .map_err(|_| "either --tenant-id or --tenant-name + --salt-path is required".to_string())?;
    let salt_path = require(flags, "salt-path")?;
    let salt = std::fs::read(salt_path)
        .map_err(|error| format!("could not read salt {salt_path}: {error}"))?;
    Ok(dev_tenant_token(&salt, tenant_name))
}

fn load_or_generate_key(path: &Path) -> std::io::Result<SigningKey> {
    if let Ok(existing) = std::fs::read(path) {
        if let Ok(seed) = <[u8; 32]>::try_from(existing.as_slice()) {
            return Ok(SigningKey::from_bytes(&seed));
        }
    }
    let mut seed = [0_u8; 32];
    getrandom::getrandom(&mut seed)
        .map_err(|error| std::io::Error::other(format!("could not generate a key: {error}")))?;
    std::fs::write(path, seed)?;
    Ok(SigningKey::from_bytes(&seed))
}

async fn run_request(flags: HashMap<String, String>) -> Result<(), String> {
    let repository_id = require(&flags, "repo")?.to_string();
    let node_id = require(&flags, "node")?.to_string();
    let tenant_id = resolve_tenant_id(&flags)?;
    let grpc_endpoint = flags
        .get("grpc-endpoint")
        .cloned()
        .unwrap_or_else(|| DEFAULT_GRPC_ENDPOINT.to_string());
    let display_name = flags
        .get("display-name")
        .cloned()
        .unwrap_or_else(|| node_id.clone());
    let capabilities: Vec<String> = flags
        .get("capability")
        .map(|value| value.split(',').map(str::to_string).collect())
        .unwrap_or_else(|| vec!["synchronize".to_string()]);

    let path = key_path(&flags, &node_id);
    let signing_key =
        load_or_generate_key(&path).map_err(|error| format!("key {}: {error}", path.display()))?;
    let public_key = signing_key.verifying_key().to_bytes().to_vec();
    let fingerprint = public_key_fingerprint(&public_key);

    let request_id = format!(
        "request-{node_id}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos()
    );

    let mut client = NodeEnrollmentServiceClient::connect(grpc_endpoint.clone())
        .await
        .map_err(|error| format!("could not reach {grpc_endpoint}: {error}"))?;
    let status = client
        .submit_enrollment_request(Request::new(v1::EnrollmentRequest {
            request_id: request_id.clone(),
            tenant_id: tenant_id.clone(),
            repository_id: repository_id.clone(),
            proposed_node_id: node_id.clone(),
            display_name,
            public_key_fingerprint: fingerprint.clone(),
            requested_capabilities: capabilities,
            created_at: now_rfc3339(),
            expires_at: "2030-01-01T00:00:00Z".to_string(),
            public_key,
        }))
        .await
        .map_err(|error| format!("submit_enrollment_request failed: {error}"))?
        .into_inner();

    let saved = SavedRequest {
        request_id: request_id.clone(),
        tenant_id: tenant_id.clone(),
        repository_id: repository_id.clone(),
        node_id: node_id.clone(),
        public_key_fingerprint: fingerprint.clone(),
        grpc_endpoint: grpc_endpoint.clone(),
    };
    let state = state_path(&path);
    std::fs::write(
        &state,
        serde_json::to_vec_pretty(&saved).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("could not write {}: {error}", state.display()))?;

    println!("submitted: {status:?}");
    println!("key saved at {}", path.display());
    println!();
    println!("A repository never approves itself (ADR-0085). Have an administrator run:");
    println!(
        "  register-me approve --request-id {request_id} --tenant-id {tenant_id} \\\n    --repo {repository_id} --fingerprint {fingerprint} \\\n    --admin-database-url <ACKPLANE_DATABASE_URL>"
    );
    println!();
    println!("Then finish this node with:");
    println!(
        "  register-me activate --request-id {request_id} --key-path {}",
        path.display()
    );
    Ok(())
}

async fn run_approve(flags: HashMap<String, String>) -> Result<(), String> {
    let request_id = require(&flags, "request-id")?.to_string();
    let tenant_id = resolve_tenant_id(&flags)?;
    let repository_id = require(&flags, "repo")?.to_string();
    let fingerprint = require(&flags, "fingerprint")?.to_string();
    let database_url = require(&flags, "admin-database-url")?.to_string();
    let approved_by = flags
        .get("approved-by")
        .cloned()
        .unwrap_or_else(|| "local-dev-admin".to_string());
    let capabilities: Vec<String> = flags
        .get("capability")
        .map(|value| value.split(',').map(str::to_string).collect())
        .unwrap_or_else(|| vec!["synchronize".to_string()]);

    println!(
        "NOTE: approving via a direct database connection ({approved_by}). This stands in for \
         the administrative approval RPC/UI ADR-0085 expects but that does not exist yet — it \
         is a single-operator developer shortcut, not how a real deployment approves nodes."
    );

    let mut store = EnrollmentStore::connect(&database_url)
        .await
        .map_err(|error| format!("could not connect to {database_url}: {error}"))?;
    let status = store
        .approve(&EnrollmentApproval {
            request_id,
            tenant_id,
            repository_id,
            public_key_fingerprint: fingerprint,
            approved_capabilities: capabilities,
            approved_by,
        })
        .await
        .map_err(|error| format!("approve failed: {error}"))?;
    println!("approved: {status:?}");
    Ok(())
}

async fn run_activate(flags: HashMap<String, String>) -> Result<(), String> {
    let request_id = require(&flags, "request-id")?.to_string();
    let path = flags
        .get("key-path")
        .map(PathBuf::from)
        .ok_or("--key-path is required (the path `register-me request` printed)")?;
    let state = state_path(&path);
    let saved: SavedRequest = serde_json::from_slice(
        &std::fs::read(&state).map_err(|error| format!("{}: {error}", state.display()))?,
    )
    .map_err(|error| format!("{}: {error}", state.display()))?;
    if saved.request_id != request_id {
        return Err(format!(
            "{} was saved for request {}, not {request_id}",
            state.display(),
            saved.request_id
        ));
    }
    let grpc_endpoint = flags
        .get("grpc-endpoint")
        .cloned()
        .unwrap_or(saved.grpc_endpoint);
    let signing_key =
        load_or_generate_key(&path).map_err(|error| format!("key {}: {error}", path.display()))?;

    let mut enrollment_client = NodeEnrollmentServiceClient::connect(grpc_endpoint.clone())
        .await
        .map_err(|error| format!("could not reach {grpc_endpoint}: {error}"))?;
    let challenge = enrollment_client
        .get_activation_challenge(Request::new(v1::EnrollmentChallengeRequest {
            request_id: request_id.clone(),
            tenant_id: saved.tenant_id.clone(),
            repository_id: saved.repository_id.clone(),
            proposed_node_id: saved.node_id.clone(),
            public_key_fingerprint: saved.public_key_fingerprint.clone(),
        }))
        .await
        .map_err(|error| format!("get_activation_challenge failed: {error}"))?
        .into_inner();
    let proof_bytes = activation_challenge_bytes(
        &challenge.nonce,
        &request_id,
        &saved.tenant_id,
        &saved.repository_id,
        &saved.node_id,
        &saved.public_key_fingerprint,
    );
    let signature = signing_key.sign(&proof_bytes).to_bytes().to_vec();
    let activation = enrollment_client
        .activate_enrollment(Request::new(v1::EnrollmentActivationProof {
            request_id: request_id.clone(),
            tenant_id: saved.tenant_id.clone(),
            repository_id: saved.repository_id.clone(),
            proposed_node_id: saved.node_id.clone(),
            public_key_fingerprint: saved.public_key_fingerprint.clone(),
            nonce: challenge.nonce.clone(),
            signature,
        }))
        .await
        .map_err(|error| format!("activate_enrollment failed: {error}"))?
        .into_inner();
    println!("activated: {activation:?}");

    if flags.contains_key("skip-sync") {
        return Ok(());
    }
    let signing_key_id = activation.signing_key_id.as_str();

    let mut sync_client = NodeSyncServiceClient::connect(grpc_endpoint)
        .await
        .map_err(|error| format!("could not open NodeSync: {error}"))?;
    let (tx, rx) = mpsc::channel::<v1::NodeFrame>(4);
    let outbound = ReceiverStream::new(rx);
    let response = sync_client
        .synchronize(Request::new(outbound))
        .await
        .map_err(|error| format!("synchronize failed: {error}"))?;
    let mut inbound = response.into_inner();

    tx.send(v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::Hello(v1::Hello {
            tenant_id: saved.tenant_id.clone(),
            repository_id: saved.repository_id.clone(),
            producer_id: saved.node_id.clone(),
            last_accepted_position: 0,
            capabilities: vec!["synchronize".to_string()],
            signing_key_id: signing_key_id.to_string(),
        })),
    })
    .await
    .map_err(|error| error.to_string())?;

    let challenge_frame = inbound
        .message()
        .await
        .map_err(|error| error.to_string())?
        .ok_or("stream closed before ConnectionChallenge")?;
    let nonce = match challenge_frame.frame {
        Some(v1::ackplane_frame::Frame::ConnectionChallenge(challenge)) => challenge.nonce,
        other => return Err(format!("expected ConnectionChallenge, got {other:?}")),
    };
    let connection_signature = signing_key
        .sign(&connection_challenge_bytes(&ConnectionChallengeBinding {
            nonce: &nonce,
            tenant_id: &saved.tenant_id,
            repository_id: &saved.repository_id,
            producer_id: &saved.node_id,
            signing_key_id,
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
    .await
    .map_err(|error| error.to_string())?;

    let accepted_frame = inbound
        .message()
        .await
        .map_err(|error| error.to_string())?
        .ok_or("stream closed before HelloAccepted")?;
    println!("hello_accepted: {accepted_frame:?}");
    let flow_control_frame = inbound
        .message()
        .await
        .map_err(|error| error.to_string())?
        .ok_or("stream closed before FlowControl")?;
    println!("flow_control: {flow_control_frame:?}");

    let payload = format!("register-me heartbeat from {}", saved.node_id).into_bytes();
    let payload_digest = Sha256::digest(&payload).to_vec();
    let mut event = v1::EventEnvelope {
        tenant_id: saved.tenant_id.clone(),
        repository_id: saved.repository_id.clone(),
        producer_id: saved.node_id.clone(),
        producer_sequence: 1,
        payload,
        payload_digest,
        schema_version: "1".to_string(),
        occurred_at: now_rfc3339(),
        payload_type: "register-me.heartbeat".to_string(),
        previous_envelope_digest: Vec::new(),
        signing_key_id: signing_key_id.to_string(),
        signature: Vec::new(),
        provenance: v1::ProvenanceClass::EnrolledNode as i32,
    };
    let signing_bytes = envelope_signing_bytes(&event);
    event.signature = signing_key.sign(&signing_bytes).to_bytes().to_vec();

    tx.send(v1::NodeFrame {
        frame: Some(v1::node_frame::Frame::EventBatch(v1::EventBatch {
            events: vec![event],
        })),
    })
    .await
    .map_err(|error| error.to_string())?;

    let receipt_frame = inbound
        .message()
        .await
        .map_err(|error| error.to_string())?
        .ok_or("stream closed before BatchReceipt")?;
    println!("batch_receipt: {receipt_frame:?}");
    println!();
    println!("{} is enrolled and synchronizing.", saved.node_id);
    Ok(())
}

fn print_usage() {
    eprintln!(
        "register-me: enroll a repository node with Ackplane (ADR-0085)\n\n\
         USAGE:\n\
         \x20 register-me request  --repo R --node N (--tenant-name T --salt-path PATH | --tenant-id ID)\n\
         \x20                      [--grpc-endpoint URL] [--key-path PATH] [--display-name NAME]\n\
         \x20                      [--capability C,C]\n\
         \x20 register-me approve  --request-id ID (--tenant-name T --salt-path PATH | --tenant-id ID)\n\
         \x20                      --repo R --fingerprint FP --admin-database-url URL\n\
         \x20                      [--approved-by NAME]\n\
         \x20 register-me activate --request-id ID --key-path PATH [--grpc-endpoint URL] [--skip-sync]\n\n\
         `--tenant-name` + `--salt-path` derive the same tenant id the Bridge queries for\n\
         (ADR-0098 decision 3) -- use it, or the enrolled repository will never appear there.\n\
         `--tenant-id` is a raw override for a deployment that assigns tenant ids some other way.\n\n\
         `request` is the only step a real node runs unattended; `approve` is a separate\n\
         administrator action (a local-dev database shortcut standing in for the approval\n\
         RPC/UI ADR-0085 does not build yet); `activate` proves possession and opens one real\n\
         NodeSync stream with a signed heartbeat event, using the signing_key_id\n\
         EnrollmentActivationResult returns directly -- no database access needed."
    );
}

#[tokio::main]
async fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        print_usage();
        return ExitCode::FAILURE;
    }
    let command = args.remove(0);
    let flags = parse_flags(&args);

    let result = match command.as_str() {
        "request" => run_request(flags).await,
        "approve" => run_approve(flags).await,
        "activate" => run_activate(flags).await,
        _ => {
            print_usage();
            return ExitCode::FAILURE;
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("register-me: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flags_reads_flag_value_pairs() {
        let args: Vec<String> = ["--repo", "r", "--node", "n"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let flags = parse_flags(&args);
        assert_eq!(flags.get("repo").map(String::as_str), Some("r"));
        assert_eq!(flags.get("node").map(String::as_str), Some("n"));
        assert_eq!(flags.len(), 2);
    }

    #[test]
    fn parse_flags_ignores_a_dangling_flag_with_no_value() {
        let args: Vec<String> = ["--repo", "r", "--dangling"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let flags = parse_flags(&args);
        assert_eq!(flags.get("repo").map(String::as_str), Some("r"));
        assert!(!flags.contains_key("dangling"));
        assert_eq!(flags.len(), 1);
    }

    #[test]
    fn require_reports_the_missing_flag_by_name() {
        let flags = HashMap::new();
        let error = require(&flags, "repo").expect_err("must be missing");
        assert_eq!(error, "--repo is required");
    }

    #[test]
    fn key_path_defaults_to_a_name_derived_from_the_node_id() {
        let flags = HashMap::new();
        assert_eq!(
            key_path(&flags, "my-node"),
            PathBuf::from("register-me-my-node.key")
        );
    }

    #[test]
    fn key_path_honors_an_explicit_override() {
        let mut flags = HashMap::new();
        flags.insert("key-path".to_string(), "/tmp/explicit.key".to_string());
        assert_eq!(
            key_path(&flags, "my-node"),
            PathBuf::from("/tmp/explicit.key")
        );
    }

    #[test]
    fn state_path_is_the_key_path_with_a_suffix() {
        assert_eq!(
            state_path(Path::new("register-me-my-node.key")),
            PathBuf::from("register-me-my-node.key.enrollment.json")
        );
    }

    #[test]
    fn dev_tenant_token_is_stable_and_not_the_bare_name() {
        let salt = b"a-fixed-test-salt";
        let first = dev_tenant_token(salt, "demo-tenant");
        let second = dev_tenant_token(salt, "demo-tenant");
        assert_eq!(first, second);
        assert_ne!(first, "demo-tenant");
        assert_eq!(first.len(), 64);
    }

    #[test]
    fn dev_tenant_token_differs_across_tenant_names_under_the_same_salt() {
        let salt = b"a-fixed-test-salt";
        assert_ne!(
            dev_tenant_token(salt, "tenant-a"),
            dev_tenant_token(salt, "tenant-b")
        );
    }

    #[test]
    fn resolve_tenant_id_prefers_an_explicit_tenant_id_override() {
        let mut flags = HashMap::new();
        flags.insert("tenant-id".to_string(), "raw-token".to_string());
        // No --tenant-name/--salt-path supplied; if the override were not
        // honoured this would fail trying to require --tenant-name.
        assert_eq!(resolve_tenant_id(&flags).unwrap(), "raw-token");
    }

    #[test]
    fn resolve_tenant_id_derives_from_name_and_salt_file() {
        let salt_path =
            std::env::temp_dir().join(format!("register-me-salt-test-{}.bin", std::process::id()));
        std::fs::write(&salt_path, b"a-fixed-test-salt").expect("write salt");

        let mut flags = HashMap::new();
        flags.insert("tenant-name".to_string(), "demo-tenant".to_string());
        flags.insert(
            "salt-path".to_string(),
            salt_path.to_string_lossy().into_owned(),
        );

        let resolved = resolve_tenant_id(&flags).expect("resolves");
        assert_eq!(
            resolved,
            dev_tenant_token(b"a-fixed-test-salt", "demo-tenant")
        );

        std::fs::remove_file(&salt_path).ok();
    }

    #[test]
    fn resolve_tenant_id_fails_without_either_form() {
        let flags = HashMap::new();
        let error = resolve_tenant_id(&flags).expect_err("must fail");
        assert_eq!(
            error,
            "either --tenant-id or --tenant-name + --salt-path is required"
        );
    }

    #[test]
    fn load_or_generate_key_persists_and_reuses_the_same_seed() {
        let path =
            std::env::temp_dir().join(format!("register-me-key-test-{}.key", std::process::id()));
        let _ = std::fs::remove_file(&path);

        let first = load_or_generate_key(&path).expect("generates a key");
        let second = load_or_generate_key(&path).expect("reuses the key");
        assert_eq!(first.to_bytes(), second.to_bytes());

        std::fs::remove_file(&path).ok();
    }
}

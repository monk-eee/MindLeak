use super::*;

pub(super) async fn run_request(flags: HashMap<String, String>) -> Result<(), String> {
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

    let path = key_path(&flags);
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
    println!("A repository never approves itself. Have an administrator run:");
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

pub(super) async fn run_approve(flags: HashMap<String, String>) -> Result<(), String> {
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
         the administrative approval RPC/UI that does not exist yet — it is a single-operator \
         developer shortcut, not how a real deployment approves nodes."
    );

    let pool = ackplane_server::db_pool::build_pool(&database_url, 1)
        .map_err(|error| format!("could not build a database pool for {database_url}: {error}"))?;
    let store = EnrollmentStore::connect(&pool)
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

pub(super) async fn run_activate(flags: HashMap<String, String>) -> Result<(), String> {
    let request_id = require(&flags, "request-id")?.to_string();
    let path = key_path(&flags);
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

    let signer = SeedSigner::new(signing_key_id, &saved.node_id, &signing_key.to_bytes());
    let mut connection = NodeSyncConnection::open(
        &grpc_endpoint,
        &signer,
        &saved.tenant_id,
        &saved.repository_id,
        vec!["synchronize".to_string()],
        0,
    )
    .await
    .map_err(|error| format!("could not open NodeSync: {error}"))?;
    println!(
        "authenticated: accepted_position={} capabilities={:?}",
        connection.accepted_position(),
        connection.enabled_capabilities()
    );

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

    let receipt = connection
        .exchange_event_batch(v1::NodeFrame {
            frame: Some(v1::node_frame::Frame::EventBatch(v1::EventBatch {
                events: vec![event],
            })),
        })
        .await
        .map_err(|error| format!("publishing the heartbeat failed: {error}"))?;
    println!("batch_receipt: {receipt:?}");
    println!();
    println!("{} is enrolled and synchronizing.", saved.node_id);
    Ok(())
}

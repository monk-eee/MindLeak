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
    let state = state_path(&path);
    match std::fs::symlink_metadata(&state) {
        Ok(_) => {
            return Err(format!(
                "saved enrollment already exists at {}; resume activation instead of creating a replacement request",
                state.display()
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{}: {error}", state.display())),
    }
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

    let channel = ackplane_client::connect_channel(&grpc_endpoint)
        .await
        .map_err(|error| format!("could not reach {grpc_endpoint}: {error}"))?;
    let mut client = NodeEnrollmentServiceClient::new(channel);
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
        activation_nonce: None,
        activation: None,
    };
    saved.save(&state)?;

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
        .map_err(|error| format!("could not build the approval database pool: {error}"))?;
    let store = EnrollmentStore::connect(&pool)
        .await
        .map_err(|error| format!("could not connect to the approval database: {error}"))?;
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
    let mut saved = SavedRequest::load(&state)?;
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
        .unwrap_or_else(|| saved.grpc_endpoint.clone());
    let signing_key =
        load_key(&path).map_err(|error| format!("key {}: {error}", path.display()))?;
    if public_key_fingerprint(&signing_key.verifying_key().to_bytes())
        != saved.public_key_fingerprint
    {
        return Err(format!(
            "key {} does not match the saved enrollment fingerprint; restore the approved key before activating",
            path.display()
        ));
    }

    if saved.activation.is_none() {
        let channel = ackplane_client::connect_channel(&grpc_endpoint)
            .await
            .map_err(|error| format!("could not reach the enrollment service: {error}"))?;
        let mut enrollment_client = NodeEnrollmentServiceClient::new(channel);
        let challenge = enrollment_client
            .get_activation_challenge(Request::new(v1::EnrollmentChallengeRequest {
                request_id: request_id.clone(),
                tenant_id: saved.tenant_id.clone(),
                repository_id: saved.repository_id.clone(),
                proposed_node_id: saved.node_id.clone(),
                public_key_fingerprint: saved.public_key_fingerprint.clone(),
            }))
            .await;
        let nonce = match challenge {
            Ok(response) => {
                let challenge = response.into_inner();
                if challenge.request_id != saved.request_id
                    || challenge.tenant_id != saved.tenant_id
                    || challenge.repository_id != saved.repository_id
                    || challenge.proposed_node_id != saved.node_id
                    || challenge.public_key_fingerprint != saved.public_key_fingerprint
                {
                    return Err(
                        "activation challenge does not match the saved enrollment".to_string()
                    );
                }
                saved.grpc_endpoint = grpc_endpoint.clone();
                saved.record_activation_nonce(&state, challenge.nonce.clone())?;
                challenge.nonce
            }
            Err(error) => match &saved.activation_nonce {
                Some(nonce) if error.code() == tonic::Code::FailedPrecondition => nonce.clone(),
                _ => return Err(format!("get_activation_challenge failed: {error}")),
            },
        };
        let proof_bytes = activation_challenge_bytes(
            &nonce,
            &request_id,
            &saved.tenant_id,
            &saved.repository_id,
            &saved.node_id,
            &saved.public_key_fingerprint,
        );
        let signature = signing_key.sign(&proof_bytes).to_bytes().to_vec();
        let response = enrollment_client
            .activate_enrollment(Request::new(v1::EnrollmentActivationProof {
                request_id: request_id.clone(),
                tenant_id: saved.tenant_id.clone(),
                repository_id: saved.repository_id.clone(),
                proposed_node_id: saved.node_id.clone(),
                public_key_fingerprint: saved.public_key_fingerprint.clone(),
                nonce,
                signature,
            }))
            .await
            .map_err(|error| format!("activate_enrollment failed: {error}"))?
            .into_inner();
        saved.grpc_endpoint = grpc_endpoint.clone();
        saved.record_activation(&state, &response)?;
    }
    let activation = saved
        .activation
        .as_ref()
        .ok_or("no recorded activation identity")?;
    println!(
        "recorded activation: signing_key_id={} enrolment_receipt_id={}; current authority is verified by NodeSync",
        activation.signing_key_id, activation.enrolment_receipt_id
    );

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

#[cfg(test)]
mod tests {
    use super::*;

    // Approval errors used to print the password-bearing URL into captured terminal output.
    #[tokio::test]
    async fn approval_errors_preserve_failure_context_without_database_credentials() {
        let password = "approval-password-must-not-be-logged";
        for (database_url, expected_context) in [
            (
                format!("postgresql://admin:{password}@127.0.0.1:invalid/enrollment"),
                "database pool",
            ),
            (
                format!("host=127.0.0.1 password={password} port=invalid"),
                "database pool",
            ),
            (
                format!("postgresql://admin:{password}@127.0.0.1:1/enrollment?connect_timeout=1"),
                "connect",
            ),
        ] {
            let flags = HashMap::from([
                ("request-id".to_string(), "request-test".to_string()),
                ("tenant-id".to_string(), "tenant-test".to_string()),
                ("repo".to_string(), "repository-test".to_string()),
                ("fingerprint".to_string(), "fingerprint-test".to_string()),
                ("admin-database-url".to_string(), database_url.clone()),
            ]);

            let error = tokio::time::timeout(std::time::Duration::from_secs(3), run_approve(flags))
                .await
                .expect("the local failure must be bounded")
                .expect_err("invalid or unreachable database must refuse approval");

            assert!(
                !error.contains(password),
                "approval error leaked a database password"
            );
            assert!(
                !error.contains(&database_url),
                "approval error repeated the connection string"
            );
            assert!(
                error.contains(expected_context),
                "approval error lost its failure category"
            );
        }
    }
}

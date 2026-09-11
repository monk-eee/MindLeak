use super::*;

pub(super) async fn run_request(flags: HashMap<String, String>) -> Result<(), String> {
    if require(&flags, "provider")? != "credential-facility-software" {
        return Err("--provider must explicitly select credential-facility-software".to_string());
    }
    let directory = state_directory(&flags)?;
    let repository_id = require(&flags, "repo")?.to_string();
    let node_id = require(&flags, "node")?.to_string();
    let tenant_id = resolve_tenant_id(&flags)?;
    let endpoint = flags
        .get("grpc-endpoint")
        .map(String::as_str)
        .unwrap_or(DEFAULT_GRPC_ENDPOINT);
    request::validate_endpoint(endpoint)?;
    let display_name = flags
        .get("display-name")
        .cloned()
        .unwrap_or_else(|| node_id.clone());
    let capabilities = flags
        .get("capability")
        .map(|value| {
            value
                .split(',')
                .map(|part| part.trim().to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec!["synchronize".to_string()]);
    if display_name.trim().is_empty() || capabilities.iter().any(String::is_empty) {
        return Err("display name and capability names must not be blank".to_string());
    }
    let path = state_path(&directory);
    let saved = match std::fs::symlink_metadata(&path) {
        Ok(_) => Some(SavedRequest::load(&path)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("could not inspect saved request: {error}")),
    };
    let candidate = provider::candidate(&tenant_id, &repository_id, &node_id, &directory)?;
    let requested = SavedRequest::new(
        &tenant_id,
        &repository_id,
        &candidate.identity(),
        endpoint,
        display_name,
        capabilities,
    )?;
    let saved = match saved {
        Some(saved) => {
            saved.ensure_matches(&requested)?;
            saved
        }
        None => {
            requested.save(&path)?;
            requested
        }
    };
    let channel = ackplane_client::connect_channel(&saved.grpc_endpoint)
        .await
        .map_err(|error| {
            format!("could not reach enrollment service; request is saved for retry: {error}")
        })?;
    let status = NodeEnrollmentServiceClient::new(channel)
        .submit_enrollment_request(Request::new(saved.request()))
        .await
        .map_err(|error| {
            format!("submit_enrollment_request failed; repeat the same request: {error}")
        })?
        .into_inner();
    if status.request_id != saved.request_id {
        return Err("enrollment service returned a different request ID".to_string());
    }
    println!("submitted: {status:?}");
    println!(
        "provider: credential-facility-software; state directory: {}",
        directory.display()
    );
    println!("A repository never approves itself. Have an administrator run:");
    println!("  register-me approve --request-id {} --tenant-id {} --repo {} --fingerprint {} --admin-database-url <ACKPLANE_DATABASE_URL>", saved.request_id, saved.tenant_id, saved.repository_id, saved.public_key_fingerprint);
    println!(
        "Then run: register-me activate --request-id {} --state-dir {:?}",
        saved.request_id, directory
    );
    Ok(())
}

pub(super) async fn run_approve(flags: HashMap<String, String>) -> Result<(), String> {
    let request_id = require(&flags, "request-id")?.to_string();
    let tenant_id = resolve_tenant_id(&flags)?;
    let repository_id = require(&flags, "repo")?.to_string();
    let fingerprint = require(&flags, "fingerprint")?.to_string();
    let database_url = require(&flags, "admin-database-url")?;
    let approved_by = flags
        .get("approved-by")
        .cloned()
        .unwrap_or_else(|| "local-dev-admin".to_string());
    let capabilities = flags
        .get("capability")
        .map(|value| value.split(',').map(str::to_string).collect())
        .unwrap_or_else(|| vec!["synchronize".to_string()]);
    println!("NOTE: direct database approval ({approved_by}) is a single-operator development shortcut, not production administrator authentication.");
    let pool = ackplane_server::db_pool::build_pool(database_url, 1)
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
    let directory = state_directory(&flags)?;
    let request_id = require(&flags, "request-id")?;
    let saved = SavedRequest::load(&state_path(&directory))?;
    if saved.request_id != request_id {
        return Err("saved enrollment belongs to a different request".to_string());
    }
    let endpoint = flags
        .get("grpc-endpoint")
        .map(String::as_str)
        .unwrap_or(&saved.grpc_endpoint);
    request::validate_endpoint(endpoint)?;
    let provider =
        match CredentialProvider::recover(&saved.tenant_id, &saved.repository_id, &directory) {
            Ok(provider) => provider,
            Err(CredentialProviderError::NotActivated) => {
                let mut candidate = CredentialCandidate::recover(
                    &saved.tenant_id,
                    &saved.repository_id,
                    &directory,
                )
                .map_err(|error| error.to_string())?;
                saved.ensure_identity(&candidate.identity())?;
                let channel = ackplane_client::connect_channel(endpoint)
                    .await
                    .map_err(|error| format!("could not reach enrollment service: {error}"))?;
                let mut client = NodeEnrollmentServiceClient::new(channel);
                let challenge = client
                    .get_activation_challenge(Request::new(v1::EnrollmentChallengeRequest {
                        request_id: saved.request_id.clone(),
                        tenant_id: saved.tenant_id.clone(),
                        repository_id: saved.repository_id.clone(),
                        proposed_node_id: saved.node_id.clone(),
                        public_key_fingerprint: saved.public_key_fingerprint.clone(),
                    }))
                    .await;
                let proof = match challenge {
                    Ok(response) => {
                        let challenge = response.into_inner();
                        if challenge.request_id != saved.request_id {
                            return Err(
                                "activation challenge names a different request".to_string()
                            );
                        }
                        candidate
                            .activation_proof(&challenge)
                            .map_err(|error| error.to_string())?
                    }
                    Err(error) if error.code() == tonic::Code::FailedPrecondition => candidate
                        .retry_activation_proof()
                        .map_err(|_| format!("get_activation_challenge failed: {error}"))?,
                    Err(error) => return Err(format!("get_activation_challenge failed: {error}")),
                };
                if proof.request_id != saved.request_id {
                    return Err("recorded activation proof names a different request".to_string());
                }
                let response = client
                    .activate_enrollment(Request::new(proof))
                    .await
                    .map_err(|error| format!("activate_enrollment failed: {error}"))?
                    .into_inner();
                candidate
                    .accept_activation(&response)
                    .map_err(|error| error.to_string())?
            }
            Err(error) => return Err(error.to_string()),
        };
    let identity = provider.identity();
    saved.ensure_identity(&ackplane_node::CandidateIdentity {
        node_id: identity.node_id.clone(),
        public_key: identity.public_key,
        fingerprint: identity.fingerprint.clone(),
    })?;
    if provider.activation().request_id != saved.request_id {
        return Err("provider activation names a different request".to_string());
    }
    println!("recorded activation: signing_key_id={} enrolment_receipt_id={}; current authority is verified by NodeSync", identity.signing_key_id, provider.activation().enrolment_receipt_id);
    if flags.contains_key("skip-sync") {
        return Ok(());
    }
    let mut connection = provider
        .open_connection(endpoint, vec!["synchronize".to_string()], 0)
        .await
        .map_err(|error| format!("could not open NodeSync: {error}"))?;
    println!(
        "authenticated: accepted_position={} capabilities={:?}",
        connection.accepted_position(),
        connection.enabled_capabilities()
    );
    let payload = format!("register-me enrollment {}", saved.request_id).into_bytes();
    let mut event = v1::EventEnvelope {
        tenant_id: saved.tenant_id.clone(),
        repository_id: saved.repository_id.clone(),
        producer_id: saved.node_id.clone(),
        producer_sequence: 1,
        payload_digest: Sha256::digest(&payload).to_vec(),
        payload,
        schema_version: "1".to_string(),
        occurred_at: saved.created_at.clone(),
        payload_type: "register-me.enrollment".to_string(),
        previous_envelope_digest: Vec::new(),
        signing_key_id: identity.signing_key_id.clone(),
        signature: Vec::new(),
        provenance: v1::ProvenanceClass::EnrolledNode as i32,
    };
    event.signature = provider
        .sign(
            "event.envelope",
            &SigningBinding {
                tenant_id: saved.tenant_id,
                repository_id: saved.repository_id,
                node_id: saved.node_id,
                key_id: identity.signing_key_id,
            },
            &envelope_signing_bytes(&event),
        )
        .map_err(|error| error.to_string())?
        .as_bytes()
        .to_vec();
    let receipt = connection
        .exchange_event_batch(v1::NodeFrame {
            frame: Some(v1::node_frame::Frame::EventBatch(v1::EventBatch {
                events: vec![event],
            })),
        })
        .await
        .map_err(|error| format!("publishing enrollment failed: {error}"))?;
    if receipt.receipts.len() != 1 {
        return Err("Ackplane did not acknowledge the enrollment event".to_string());
    }
    println!("batch_receipt: {receipt:?}");
    Ok(())
}

#[cfg(test)]
mod tests;

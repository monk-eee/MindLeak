use std::{io::Write, sync::Arc};

use super::*;

pub(super) async fn run(flags: HashMap<String, String>) -> Result<(), String> {
    let directory = state_directory(&flags)?;
    let saved = SavedRequest::load(&state_path(&directory))?;
    let endpoint = flags
        .get("grpc-endpoint")
        .cloned()
        .unwrap_or_else(|| saved.grpc_endpoint.clone());
    request::validate_endpoint(&endpoint)?;
    let provider = CredentialProvider::recover(&saved.tenant_id, &saved.repository_id, &directory)
        .map_err(|error| error.to_string())?;
    let identity = provider.identity();
    saved.ensure_identity(&ackplane_node::CandidateIdentity {
        node_id: identity.node_id,
        public_key: identity.public_key,
        fingerprint: identity.fingerprint,
    })?;
    if provider.activation().request_id != saved.request_id {
        return Err("provider activation does not match the saved request".to_string());
    }
    let service = Arc::new(
        ackplane_node::companion::NodeService::new(
            saved.tenant_id,
            saved.repository_id,
            endpoint,
            Arc::new(provider),
        )
        .map_err(|error| error.to_string())?,
    );
    service
        .verify_authority()
        .await
        .map_err(|error| error.to_string())?;
    let listener = service
        .bind(&directory)
        .map_err(|error| error.to_string())?;
    println!("node companion ready: {}", directory.display());
    std::io::stdout()
        .flush()
        .map_err(|error| error.to_string())?;
    service
        .serve(listener, async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|error| error.to_string())
}

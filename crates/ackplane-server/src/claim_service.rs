//! gRPC transport for Ackplane-authoritative delegated task claim leases.

use std::{sync::Arc, time::Duration};

use ackplane_protocol::v1;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use crate::claim_store::{ClaimLeaseOutcome, ClaimLeaseRequest, ClaimStore, ClaimStoreError};

pub struct ClaimDelegationService {
    store: Arc<Mutex<ClaimStore>>,
}

impl ClaimDelegationService {
    pub fn new(store: ClaimStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }
}

#[tonic::async_trait]
impl v1::claim_delegation_service_server::ClaimDelegationService for ClaimDelegationService {
    async fn delegate_claim(
        &self,
        request: Request<v1::ClaimLeaseRequest>,
    ) -> Result<Response<v1::ClaimLeaseResult>, Status> {
        let request = request_from_wire(request.into_inner()).map_err(Status::invalid_argument)?;
        let result = self
            .store
            .lock()
            .await
            .delegate(&request, std::time::SystemTime::now())
            .await
            .map_err(map_store_error)?;
        Ok(Response::new(
            result_to_wire(result).map_err(Status::internal)?,
        ))
    }

    async fn release_claim(
        &self,
        request: Request<v1::ClaimReleaseRequest>,
    ) -> Result<Response<v1::ClaimReleaseResult>, Status> {
        let request = request.into_inner();
        let tenant_id =
            required(request.tenant_id, "tenant_id").map_err(Status::invalid_argument)?;
        let repository_id =
            required(request.repository_id, "repository_id").map_err(Status::invalid_argument)?;
        let task_id = required(request.task_id, "task_id").map_err(Status::invalid_argument)?;
        let owner_id = required(request.owner_id, "owner_id").map_err(Status::invalid_argument)?;
        let released = self
            .store
            .lock()
            .await
            .release(
                &tenant_id,
                &repository_id,
                &task_id,
                &owner_id,
                std::time::SystemTime::now(),
            )
            .await
            .map_err(map_store_error)?;
        Ok(Response::new(v1::ClaimReleaseResult {
            released,
            diagnostic: String::new(),
        }))
    }
}

fn request_from_wire(request: v1::ClaimLeaseRequest) -> Result<ClaimLeaseRequest, String> {
    let lease = Duration::from_secs(request.lease_seconds);
    if lease.is_zero() {
        return Err("lease_seconds must be greater than zero".to_owned());
    }
    Ok(ClaimLeaseRequest {
        tenant_id: required(request.tenant_id, "tenant_id")?,
        repository_id: required(request.repository_id, "repository_id")?,
        task_id: required(request.task_id, "task_id")?,
        owner_id: required(request.owner_id, "owner_id")?,
        branch: required(request.branch, "branch")?,
        lease,
        paths: request.paths,
        symbols: request.symbols,
    })
}

fn result_to_wire(
    result: crate::claim_store::ClaimLeaseResult,
) -> Result<v1::ClaimLeaseResult, String> {
    Ok(v1::ClaimLeaseResult {
        outcome: match result.outcome {
            ClaimLeaseOutcome::Granted => v1::ClaimLeaseOutcome::Granted,
            ClaimLeaseOutcome::Rejected => v1::ClaimLeaseOutcome::Rejected,
        } as i32,
        owner_id: result.owner_id,
        branch: result.branch,
        claim_started_at: rfc3339(result.claim_started_at)?,
        lease_expires_at: rfc3339(result.lease_expires_at)?,
        claim_lapses: result.claim_lapses,
        paths: result.paths,
        symbols: result.symbols,
        diagnostic: String::new(),
    })
}

fn required(value: String, field: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        Err(format!("{field} is required"))
    } else {
        Ok(value)
    }
}

fn rfc3339(timestamp: std::time::SystemTime) -> Result<String, String> {
    time::OffsetDateTime::from(timestamp)
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| format!("could not format claim lease timestamp: {error}"))
}

fn map_store_error(error: ClaimStoreError) -> Status {
    match error {
        ClaimStoreError::InvalidLease => Status::invalid_argument(error.to_string()),
        ClaimStoreError::Database(_) | ClaimStoreError::InvalidLapseCount => {
            Status::internal(error.to_string())
        }
    }
}

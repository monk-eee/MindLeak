//! gRPC transport for Ackplane-authoritative delegated task claim leases.

use std::{sync::Arc, time::Duration, time::SystemTime};

use ackplane_protocol::v1;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use crate::claim_signature::{self, ClaimAuthRefusal};
use crate::claim_store::{
    ClaimLeaseOutcome, ClaimLeaseRequest, ClaimRecoverRequest, ClaimStore, ClaimStoreError,
};

pub struct ClaimDelegationService {
    store: Arc<Mutex<ClaimStore>>,
}

impl ClaimDelegationService {
    pub fn new(store: ClaimStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }

    /// Verify a claim request's authentication before it reaches the store's
    /// CAS logic (ADR-0096 clause 4's authentication gap). An absent,
    /// unresolvable, mismatched-binding, not-yet-active, expired, retired, or
    /// revoked key is refused here -- the CAS methods never see an
    /// unauthenticated caller.
    async fn authenticate(
        &self,
        tenant_id: &str,
        repository_id: &str,
        task_id: &str,
        owner_id: &str,
        authentication: Option<&v1::ClaimAuthentication>,
    ) -> Result<(), Status> {
        let Some(authentication) = authentication else {
            return Err(Status::unauthenticated(
                ClaimAuthRefusal::Unsigned.diagnostic(),
            ));
        };
        let binding = crate::signing_keys::EnvelopeBinding {
            signing_key_id: &authentication.signing_key_id,
            tenant_id,
            repository_id,
            producer_id: &authentication.node_id,
            accepted_at: SystemTime::now(),
        };
        let resolution = self
            .store
            .lock()
            .await
            .resolve_signing_key(&binding)
            .await
            .map_err(|error| Status::internal(error.to_string()))?;

        claim_signature::verify(
            tenant_id,
            repository_id,
            task_id,
            owner_id,
            Some(authentication),
            &resolution,
        )
        .map_err(|refusal| {
            if refusal.is_authenticated_but_not_authorized() {
                Status::permission_denied(refusal.diagnostic())
            } else {
                Status::unauthenticated(refusal.diagnostic())
            }
        })
    }
}

#[tonic::async_trait]
impl v1::claim_delegation_service_server::ClaimDelegationService for ClaimDelegationService {
    async fn delegate_claim(
        &self,
        request: Request<v1::ClaimLeaseRequest>,
    ) -> Result<Response<v1::ClaimLeaseResult>, Status> {
        let wire = request.into_inner();
        self.authenticate(
            &wire.tenant_id,
            &wire.repository_id,
            &wire.task_id,
            &wire.owner_id,
            wire.authentication.as_ref(),
        )
        .await?;
        let request = request_from_wire(wire).map_err(Status::invalid_argument)?;
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
        self.authenticate(
            &tenant_id,
            &repository_id,
            &task_id,
            &owner_id,
            request.authentication.as_ref(),
        )
        .await?;
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

    async fn renew_claim(
        &self,
        request: Request<v1::ClaimRenewRequest>,
    ) -> Result<Response<v1::ClaimLeaseResult>, Status> {
        let request = request.into_inner();
        let tenant_id =
            required(request.tenant_id, "tenant_id").map_err(Status::invalid_argument)?;
        let repository_id =
            required(request.repository_id, "repository_id").map_err(Status::invalid_argument)?;
        let task_id = required(request.task_id, "task_id").map_err(Status::invalid_argument)?;
        let owner_id = required(request.owner_id, "owner_id").map_err(Status::invalid_argument)?;
        self.authenticate(
            &tenant_id,
            &repository_id,
            &task_id,
            &owner_id,
            request.authentication.as_ref(),
        )
        .await?;
        let lease = Duration::from_secs(request.lease_seconds);
        if lease.is_zero() {
            return Err(Status::invalid_argument(
                "lease_seconds must be greater than zero",
            ));
        }
        let result = self
            .store
            .lock()
            .await
            .renew(
                &tenant_id,
                &repository_id,
                &task_id,
                &owner_id,
                lease,
                std::time::SystemTime::now(),
            )
            .await
            .map_err(map_store_error)?;
        Ok(Response::new(
            result_to_wire(result).map_err(Status::internal)?,
        ))
    }

    async fn recover_claim(
        &self,
        request: Request<v1::ClaimRecoverRequest>,
    ) -> Result<Response<v1::ClaimLeaseResult>, Status> {
        let request = request.into_inner();
        let tenant_id =
            required(request.tenant_id, "tenant_id").map_err(Status::invalid_argument)?;
        let repository_id =
            required(request.repository_id, "repository_id").map_err(Status::invalid_argument)?;
        let task_id = required(request.task_id, "task_id").map_err(Status::invalid_argument)?;
        let expected_owner =
            required(request.expected_owner, "expected_owner").map_err(Status::invalid_argument)?;
        let owner_id = required(request.owner_id, "owner_id").map_err(Status::invalid_argument)?;
        let branch = required(request.branch, "branch").map_err(Status::invalid_argument)?;
        self.authenticate(
            &tenant_id,
            &repository_id,
            &task_id,
            &owner_id,
            request.authentication.as_ref(),
        )
        .await?;
        let lease = Duration::from_secs(request.lease_seconds);
        if lease.is_zero() {
            return Err(Status::invalid_argument(
                "lease_seconds must be greater than zero",
            ));
        }
        let result = self
            .store
            .lock()
            .await
            .recover(
                &ClaimRecoverRequest {
                    tenant_id,
                    repository_id,
                    task_id,
                    expected_owner,
                    owner_id,
                    reason: request.reason,
                    branch,
                    lease,
                    paths: request.paths,
                    symbols: request.symbols,
                },
                std::time::SystemTime::now(),
            )
            .await
            .map_err(map_store_error)?;
        Ok(Response::new(
            result_to_wire(result).map_err(Status::internal)?,
        ))
    }

    async fn list_active_claims(
        &self,
        request: Request<v1::ActiveClaimsRequest>,
    ) -> Result<Response<v1::ActiveClaimsResult>, Status> {
        let request = request.into_inner();
        let tenant_id =
            required(request.tenant_id, "tenant_id").map_err(Status::invalid_argument)?;
        let repository_id =
            required(request.repository_id, "repository_id").map_err(Status::invalid_argument)?;
        let claims = self
            .store
            .lock()
            .await
            .list_active(&tenant_id, &repository_id, std::time::SystemTime::now())
            .await
            .map_err(map_store_error)?;
        let claims = claims
            .into_iter()
            .map(active_claim_to_wire)
            .collect::<Result<Vec<_>, String>>()
            .map_err(Status::internal)?;
        Ok(Response::new(v1::ActiveClaimsResult { claims }))
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

fn active_claim_to_wire(
    claim: crate::claim_store::ActiveClaim,
) -> Result<v1::ActiveClaimSummary, String> {
    Ok(v1::ActiveClaimSummary {
        task_id: claim.task_id,
        owner_id: claim.owner_id,
        branch: claim.branch,
        lease_expires_at: rfc3339(claim.lease_expires_at)?,
        paths: claim.paths,
        symbols: claim.symbols,
    })
}

fn rfc3339(timestamp: std::time::SystemTime) -> Result<String, String> {
    time::OffsetDateTime::from(timestamp)
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| format!("could not format claim lease timestamp: {error}"))
}

fn map_store_error(error: ClaimStoreError) -> Status {
    match error {
        ClaimStoreError::InvalidLease | ClaimStoreError::MissingReason => {
            Status::invalid_argument(error.to_string())
        }
        ClaimStoreError::Database(_) | ClaimStoreError::InvalidLapseCount => {
            Status::internal(error.to_string())
        }
    }
}

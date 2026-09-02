//! Wire-shape conversions between `ackplane-server`'s in-process
//! `claim_store` types and the `ClaimDelegationService` protobuf messages --
//! split out to keep `mod.rs`'s trait implementation under the 450-line
//! module-length ratchet after ADR-0096's park/answer clause completion.

use ackplane_protocol::v1;
use tonic::Status;

use crate::claim_store::{ClaimLeaseOutcome, ClaimLeaseRequest, ClaimStoreError};

pub(super) fn request_from_wire(
    request: v1::ClaimLeaseRequest,
) -> Result<ClaimLeaseRequest, String> {
    let lease = std::time::Duration::from_secs(request.lease_seconds);
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

pub(super) fn result_to_wire(
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

pub(super) fn required(value: String, field: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        Err(format!("{field} is required"))
    } else {
        Ok(value)
    }
}

pub(super) fn active_claim_to_wire(
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
    crate::wire_format::rfc3339(timestamp)
        .map_err(|error| format!("could not format claim lease timestamp: {error}"))
}

pub(super) fn map_store_error(error: ClaimStoreError) -> Status {
    match error {
        ClaimStoreError::InvalidLease | ClaimStoreError::MissingReason => {
            Status::invalid_argument(error.to_string())
        }
        // Exhaustion is the caller's to retry, not a bad request and not a
        // permanent fault: `unavailable` is the one gRPC code that says so
        // (ADR-0143 decision 5).
        ClaimStoreError::PoolExhausted(_) => Status::unavailable(error.to_string()),
        ClaimStoreError::Database(_)
        | ClaimStoreError::SigningKey(_)
        | ClaimStoreError::InvalidLapseCount => Status::internal(error.to_string()),
    }
}

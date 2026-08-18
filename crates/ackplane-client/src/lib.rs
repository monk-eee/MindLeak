//! A repository-side client for Ackplane's delegated claim-lease contract
//! (ADR-0096). Speaks the pinned `ackplane-protocol` wire contract over gRPC
//! to a running `ackplane-server`.
//!
//! Deliberately depends on neither `mindleak-core` nor `lodestar-core`
//! (ADR-0082 clause 1 forbids a service crate depending on a plane crate;
//! this crate sits on the repository side of that same boundary and must not
//! smuggle a plane dependency back into a client a plane optionally embeds).
//!
//! What this crate does not do: it cannot yet tell "the arbiter is
//! unreachable" apart from "the arbiter is reachable but does not recognise
//! this repository", because today's wire contract has no RPC that answers
//! an enrolment question — see
//! `gaps.d/ackplane-client-cannot-detect-unenrolled-repositories.md`.

use std::time::Duration;

use ackplane_protocol::v1::claim_delegation_service_client::ClaimDelegationServiceClient;
use thiserror::Error;
use tonic::transport::Channel;

pub mod auth;
pub use auth::{authenticate, ClaimOperation, ClaimSigner, SeedSigner};

pub use ackplane_protocol::v1::{
    ClaimLeaseOutcome, ClaimLeaseRequest, ClaimLeaseResult, ClaimRecoverRequest,
    ClaimReleaseRequest, ClaimReleaseResult, ClaimRenewRequest,
};

/// How long a connection attempt is given before it counts as unreachable.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Failure connecting to, or talking with, an Ackplane deployment.
#[derive(Debug, Error)]
pub enum ClientError {
    #[error("`{0}` is not a valid Ackplane endpoint URI")]
    InvalidEndpoint(String),
    #[error("could not reach the Ackplane arbiter: {0}")]
    Unreachable(#[from] tonic::transport::Error),
    #[error("Ackplane rejected the request: {0}")]
    Rejected(#[from] tonic::Status),
}

/// A live connection to one Ackplane deployment's claim-delegation service.
pub struct ClaimClient {
    inner: ClaimDelegationServiceClient<Channel>,
}

impl ClaimClient {
    /// Connect to `endpoint` (e.g. `http://127.0.0.1:8443`), refusing rather
    /// than blocking indefinitely if the arbiter never answers.
    pub async fn connect(endpoint: &str) -> Result<Self, ClientError> {
        let channel = Channel::from_shared(endpoint.to_string())
            .map_err(|_| ClientError::InvalidEndpoint(endpoint.to_string()))?
            .connect_timeout(CONNECT_TIMEOUT)
            .connect()
            .await?;
        Ok(Self {
            inner: ClaimDelegationServiceClient::new(channel),
        })
    }

    /// Ask Ackplane to grant or refuse a lease over a task's scope
    /// (`ClaimDelegationService.DelegateClaim`).
    pub async fn delegate_claim(
        &mut self,
        request: ClaimLeaseRequest,
    ) -> Result<ClaimLeaseResult, ClientError> {
        Ok(self.inner.delegate_claim(request).await?.into_inner())
    }

    /// Extend an already-granted lease (`ClaimDelegationService.RenewClaim`).
    pub async fn renew_claim(
        &mut self,
        request: ClaimRenewRequest,
    ) -> Result<ClaimLeaseResult, ClientError> {
        Ok(self.inner.renew_claim(request).await?.into_inner())
    }

    /// Hole a lease immediately (`ClaimDelegationService.ReleaseClaim`).
    pub async fn release_claim(
        &mut self,
        request: ClaimReleaseRequest,
    ) -> Result<ClaimReleaseResult, ClientError> {
        Ok(self.inner.release_claim(request).await?.into_inner())
    }

    /// Recover a lapsed or stranded lease (`ClaimDelegationService.RecoverClaim`).
    pub async fn recover_claim(
        &mut self,
        request: ClaimRecoverRequest,
    ) -> Result<ClaimLeaseResult, ClientError> {
        Ok(self.inner.recover_claim(request).await?.into_inner())
    }
}

/// Whether an Ackplane deployment at `endpoint` answers at all.
///
/// This is a transport-level reachability check, not an enrolment check: a
/// `true` result means a `federated` repository has an arbiter to talk to,
/// not that this repository is the one it recognises.
pub async fn probe_reachable(endpoint: &str) -> bool {
    ClaimClient::connect(endpoint).await.is_ok()
}

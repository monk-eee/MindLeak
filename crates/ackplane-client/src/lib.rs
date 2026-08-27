//! A repository-side client for Ackplane's delegated claim-lease contract
//! (ADR-0096). Speaks the pinned `ackplane-protocol` wire contract over gRPC
//! to a running `ackplane-server`.
//!
//! Deliberately depends on neither `mindleak-core` nor `lodestar-core`
//! (ADR-0082 clause 1 forbids a service crate depending on a plane crate;
//! this crate sits on the repository side of that same boundary and must not
//! smuggle a plane dependency back into a client a plane optionally embeds).
//!
//! `EnrollmentClient::check_enrollment_status` asks ADR-0122's
//! `CheckEnrollmentStatus` RPC. The [`identity`] module sources and
//! persists the candidate identity and signing key that request's
//! `authentication` field needs, for CLI-side bootstrapping
//! (`register-me`), narrowing
//! `gaps.d/ackplane-client-cannot-detect-unenrolled-repositories.md`. It is
//! deliberately not wired into any long-running local plane's own startup
//! readiness check -- that would mean the plane loading and signing with
//! this repository's raw private key itself, which ADR-0100 decision 3
//! reserves for the `ackplane-node` companion's non-exporting signer.

// Every fallible call here returns `Result<_, ClientError>`, and
// `ClientError::Rejected` wraps `tonic::Status` -- a type this crate does not
// define, not a choice any one method here makes. Boxing it would mean
// boxing a type the client only ever passes through from `tonic`, so this is
// a lint to silence crate-wide (mirrors the same allow on ackplane-protocol
// and ackplane-server, which return the identical status type).
#![allow(clippy::result_large_err)]

use std::time::Duration;

use ackplane_protocol::v1::claim_delegation_service_client::ClaimDelegationServiceClient;
use ackplane_protocol::v1::node_enrollment_service_client::NodeEnrollmentServiceClient;
use thiserror::Error;
use tonic::transport::Channel;

pub mod auth;
pub use auth::{
    authenticate, decode_seed, encode_seed, ClaimOperation, ClaimSigner, CredentialFacilityError,
    CredentialFacilitySigner, SeedSigner,
};

pub mod identity;
pub use identity::{
    load_candidate_identity, resolve_key_path, signed_status_request, CandidateIdentity,
    IdentityError, DEFAULT_KEY_PATH, KEY_PATH_ENV,
};

pub mod node_sync;
pub use node_sync::NodeSyncConnection;

pub use ackplane_protocol::v1::{
    ClaimLeaseOutcome, ClaimLeaseRequest, ClaimLeaseResult, ClaimRecoverRequest,
    ClaimReleaseRequest, ClaimReleaseResult, ClaimRenewRequest, EnrollmentState,
    EnrollmentStatusAuthentication, EnrollmentStatusRequest, EnrollmentStatusResult,
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
    /// The `Synchronize` stream closed before completing the connection
    /// handshake (Hello -> ConnectionChallenge -> ChallengeResponse ->
    /// HelloAccepted -> FlowControl) -- distinct from [`Self::Rejected`],
    /// which carries a gRPC-level status, and from
    /// [`Self::ConnectionRefused`], which carries the server's own typed
    /// application-level reason.
    #[error("the Synchronize stream closed before completing the connection handshake")]
    HandshakeStreamClosed,
    /// The server sent a frame this step of the handshake was not expecting.
    /// Naming both `expected` and `got` here, rather than only logging them,
    /// is what lets a caller tell a protocol violation apart from a
    /// transport failure.
    #[error("expected {expected} during the connection handshake, got {got}")]
    UnexpectedHandshakeFrame {
        expected: &'static str,
        got: &'static str,
    },
    /// Ackplane's own application-level refusal of the whole connection
    /// (ADR-0098 decision 1), e.g. an unknown `signing_key_id`, a bad
    /// challenge-response signature, or a revoked key. Distinguishable from a
    /// bare `tonic::Status` because the caller needs the typed reason, not
    /// only a string.
    #[error("Ackplane refused this connection ({reason:?}, retryable={retryable}): {diagnostic}")]
    ConnectionRefused {
        reason: ackplane_protocol::v1::RejectionReason,
        retryable: bool,
        diagnostic: String,
    },
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

/// A live connection to one Ackplane deployment's node enrollment service.
///
/// Narrows `gaps.d/ackplane-client-cannot-detect-unenrolled-repositories.md`:
/// this crate can now ask ADR-0122's `CheckEnrollmentStatus`, and the
/// [`identity`] module sources and persists the candidate identity and
/// signing key a caller needs to build that request. This type only
/// carries an already-built, already-signed request to the wire -- it does
/// not decide who calls it or when, which is exactly the boundary ADR-0100
/// draws between a CLI bootstrapper and any long-running local plane.
pub struct EnrollmentClient {
    inner: NodeEnrollmentServiceClient<Channel>,
}

impl EnrollmentClient {
    /// Connect to `endpoint` (e.g. `http://127.0.0.1:8443`), refusing rather
    /// than blocking indefinitely if the arbiter never answers.
    pub async fn connect(endpoint: &str) -> Result<Self, ClientError> {
        let channel = Channel::from_shared(endpoint.to_string())
            .map_err(|_| ClientError::InvalidEndpoint(endpoint.to_string()))?
            .connect_timeout(CONNECT_TIMEOUT)
            .connect()
            .await?;
        Ok(Self {
            inner: NodeEnrollmentServiceClient::new(channel),
        })
    }

    /// Ask whether a candidate (node, key) binding is enrolled right now
    /// (`NodeEnrollmentService.CheckEnrollmentStatus`). Always `Ok` on a
    /// reachable arbiter -- ADR-0122 collapses every verification failure
    /// into `EnrollmentStatusResult { verified: false, .. }` rather than a
    /// distinguishable error, so `Err` here means the arbiter itself could
    /// not be reached, not that the binding was refused.
    pub async fn check_enrollment_status(
        &mut self,
        request: EnrollmentStatusRequest,
    ) -> Result<EnrollmentStatusResult, ClientError> {
        Ok(self
            .inner
            .check_enrollment_status(request)
            .await?
            .into_inner())
    }
}

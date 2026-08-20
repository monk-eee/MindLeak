//! gRPC transport for Ackplane-authoritative delegated task claim leases.

use std::{sync::Arc, time::Duration, time::SystemTime};

use ackplane_protocol::claim_auth::ClaimOperation;
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
        operation: &ClaimOperation<'_>,
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
            operation,
            Some(authentication),
            &resolution,
            SystemTime::now(),
        )
        .map_err(|refusal| {
            if refusal.is_authenticated_but_not_authorized() {
                Status::permission_denied(refusal.diagnostic())
            } else {
                Status::unauthenticated(refusal.diagnostic())
            }
        })?;

        // Only after a genuine signature is confirmed: a forged request must
        // never be able to burn a legitimate nonce out from under its owner.
        let fresh = self
            .store
            .lock()
            .await
            .consume_claim_nonce(
                &authentication.signing_key_id,
                &authentication.nonce,
                SystemTime::now(),
            )
            .await
            .map_err(|error| Status::internal(error.to_string()))?;
        if !fresh {
            return Err(Status::unauthenticated(
                ClaimAuthRefusal::Replayed.diagnostic(),
            ));
        }
        Ok(())
    }
}

#[tonic::async_trait]
impl v1::claim_delegation_service_server::ClaimDelegationService for ClaimDelegationService {
    async fn delegate_claim(
        &self,
        request: Request<v1::ClaimLeaseRequest>,
    ) -> Result<Response<v1::ClaimLeaseResult>, Status> {
        let wire = request.into_inner();
        let operation = ClaimOperation::Delegate {
            branch: &wire.branch,
            lease_seconds: wire.lease_seconds,
            paths: &wire.paths,
            symbols: &wire.symbols,
        };
        self.authenticate(
            &wire.tenant_id,
            &wire.repository_id,
            &wire.task_id,
            &wire.owner_id,
            &operation,
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
            &ClaimOperation::Release,
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
            &ClaimOperation::Renew {
                lease_seconds: request.lease_seconds,
            },
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
            &ClaimOperation::Recover {
                expected_owner: &expected_owner,
                branch: &branch,
                lease_seconds: request.lease_seconds,
                paths: &request.paths,
                symbols: &request.symbols,
                reason: &request.reason,
            },
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

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};
    use tokio_postgres::NoTls;

    // The trait's simple name collides with this module's `struct
    // ClaimDelegationService`; `as _` brings its methods into scope for
    // direct method-call syntax without trying to bind the colliding name.
    use ackplane_protocol::v1::claim_delegation_service_server::ClaimDelegationService as _;

    use super::*;
    use crate::signing_keys::{self, SigningKeyRecord};

    /// Deterministic key material across every test -- like `arbitration.rs`'s
    /// fixture -- but each test registers it under its own fresh identity
    /// (see `TestIdentity::fresh`), so two tests in the same binary can never
    /// collide on `signing_keys`' secondary uniqueness constraint over
    /// `(tenant_id, repository_id, node_id, public_key_fingerprint,
    /// activated_at)`.
    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[21; 32])
    }

    /// One test's fully-isolated tenant/repository/node/key identity, so
    /// tests in the same binary never share a row and never depend on
    /// registration order.
    struct TestIdentity {
        signing_key_id: String,
        node_id: String,
        tenant_id: String,
        repository_id: String,
    }

    impl TestIdentity {
        fn fresh(label: &str) -> Self {
            let suffix = crate::test_support::uuid_ish();
            Self {
                signing_key_id: format!("claim-service-{label}-key-{suffix}"),
                node_id: format!("claim-service-{label}-node-{suffix}"),
                tenant_id: format!("claim-service-{label}-tenant-{suffix}"),
                repository_id: format!("claim-service-{label}-repository-{suffix}"),
            }
        }
    }

    async fn register_test_key(database_url: &str, identity: &TestIdentity) {
        let (mut client, connection) = tokio_postgres::connect(database_url, NoTls)
            .await
            .expect("the gated test database should accept a signing-key connection");
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let transaction = client
            .transaction()
            .await
            .expect("a transaction should open for key registration");
        let key = signing_key();
        signing_keys::register(
            &transaction,
            &SigningKeyRecord {
                signing_key_id: identity.signing_key_id.clone(),
                tenant_id: identity.tenant_id.clone(),
                repository_id: identity.repository_id.clone(),
                node_id: identity.node_id.clone(),
                public_key: key.verifying_key().to_bytes().to_vec(),
                public_key_fingerprint: identity.signing_key_id.clone(),
                activated_at: SystemTime::UNIX_EPOCH,
                expires_at: None,
            },
        )
        .await
        .expect("registering the test key should succeed");
        transaction
            .commit()
            .await
            .expect("the registration transaction should commit");
    }

    /// A validly-signed wire request over `task_id`/`owner_id`, `signed_at`
    /// pinned to "now" and `nonce` distinguished by the caller so two
    /// requests in the same test can be deliberately identical or distinct.
    fn authenticated_request(
        identity: &TestIdentity,
        task_id: &str,
        owner_id: &str,
        nonce_byte: u8,
    ) -> v1::ClaimLeaseRequest {
        let key = signing_key();
        let branch = format!("branch/{owner_id}");
        let paths = vec!["src/lib.rs".to_owned()];
        let symbols: Vec<String> = vec![];
        let lease_seconds = 60;
        let mut authentication = v1::ClaimAuthentication {
            signing_key_id: identity.signing_key_id.clone(),
            node_id: identity.node_id.clone(),
            signed_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap(),
            nonce: vec![nonce_byte; 16],
            signature: Vec::new(),
        };
        let operation = ClaimOperation::Delegate {
            branch: &branch,
            lease_seconds,
            paths: &paths,
            symbols: &symbols,
        };
        let bytes = claim_signature::claim_signing_bytes(
            &identity.tenant_id,
            &identity.repository_id,
            task_id,
            owner_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        v1::ClaimLeaseRequest {
            tenant_id: identity.tenant_id.clone(),
            repository_id: identity.repository_id.clone(),
            task_id: task_id.to_owned(),
            owner_id: owner_id.to_owned(),
            branch,
            lease_seconds,
            paths,
            symbols,
            authentication: Some(authentication),
        }
    }

    /// Proves `authenticate` actually wires nonce consumption into the RPC
    /// path: the identical wire request granted the first time is refused
    /// the second time on the same (signing_key_id, nonce) pair. Without
    /// this, a captured `delegate_claim` request stays replayable forever --
    /// a same-owner renewal looks legitimate at the CAS layer, which is
    /// exactly why the refusal has to happen before the CAS ever sees it.
    #[tokio::test]
    async fn an_identical_request_is_granted_once_then_refused_as_replayed() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("replay");
        register_test_key(&database_url, &identity).await;
        let store = ClaimStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a claim-store connection");
        let service = ClaimDelegationService::new(store);
        let wire = authenticated_request(&identity, "task", "owner-a", 91);

        let granted = service
            .delegate_claim(Request::new(wire.clone()))
            .await
            .expect("the first, fresh request must be authenticated and granted");
        assert_eq!(
            granted.into_inner().outcome,
            v1::ClaimLeaseOutcome::Granted as i32
        );

        let replayed = service
            .delegate_claim(Request::new(wire))
            .await
            .expect_err("the identical (signing_key_id, nonce) pair must be refused");
        assert_eq!(replayed.code(), tonic::Code::Unauthenticated);
        assert!(
            replayed.message().contains("already been used"),
            "unexpected diagnostic: {}",
            replayed.message()
        );
    }

    /// A `signed_at` far outside the accepted clock-skew window is refused
    /// before the request ever reaches the nonce store or the CAS --
    /// freshness protects a captured signature from staying usable
    /// indefinitely, independent of whether its nonce has been seen before.
    #[tokio::test]
    async fn a_stale_signed_at_is_refused_before_the_cas_runs() {
        let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
            println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
            return;
        };
        let identity = TestIdentity::fresh("stale");
        register_test_key(&database_url, &identity).await;
        let store = ClaimStore::connect(&database_url)
            .await
            .expect("the gated test database should accept a claim-store connection");
        let service = ClaimDelegationService::new(store);
        let mut wire = authenticated_request(&identity, "task", "owner-a", 92);
        // Re-sign over a `signed_at` far outside the skew window -- the
        // signature must cover the stale timestamp, or this would only prove
        // the diagnostic string exists, not that verification used it.
        let key = signing_key();
        let mut authentication = wire.authentication.take().unwrap();
        authentication.signed_at = "2020-01-01T00:00:00Z".to_owned();
        let operation = ClaimOperation::Delegate {
            branch: &wire.branch,
            lease_seconds: wire.lease_seconds,
            paths: &wire.paths,
            symbols: &wire.symbols,
        };
        let bytes = claim_signature::claim_signing_bytes(
            &wire.tenant_id,
            &wire.repository_id,
            &wire.task_id,
            &wire.owner_id,
            &operation,
            &authentication,
        );
        authentication.signature = key.sign(&bytes).to_bytes().to_vec();
        wire.authentication = Some(authentication);

        let refused = service
            .delegate_claim(Request::new(wire))
            .await
            .expect_err("a signed_at far outside the skew window must be refused");
        assert_eq!(refused.code(), tonic::Code::Unauthenticated);
        assert!(
            refused.message().contains("clock-skew"),
            "unexpected diagnostic: {}",
            refused.message()
        );
    }
}

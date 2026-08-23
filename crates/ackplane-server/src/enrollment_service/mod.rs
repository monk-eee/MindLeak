//! gRPC transport for Ackplane's node enrollment authority (ADR-0085).

use std::{sync::Arc, time::SystemTime};

use ackplane_protocol::v1;
use ed25519_dalek::VerifyingKey;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use crate::{
    enrollment::{public_key_fingerprint, EnrollmentState},
    enrollment_status_signature,
    enrollment_store::{
        ActivationChallengeRequest, EnrollmentActivation, EnrollmentStatus, EnrollmentStore,
        EnrollmentStoreError, EnrollmentSubmission, KeyRotation, KeyRotationOutcome,
        KeyRotationRejection,
    },
};

/// The gRPC enrollment authority. Database access is serialized because the
/// store owns one PostgreSQL client and each authority operation is atomic.
pub struct NodeEnrollmentService {
    store: Arc<Mutex<EnrollmentStore>>,
}

impl NodeEnrollmentService {
    pub fn new(store: EnrollmentStore) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
        }
    }
}

#[tonic::async_trait]
impl v1::node_enrollment_service_server::NodeEnrollmentService for NodeEnrollmentService {
    async fn submit_enrollment_request(
        &self,
        request: Request<v1::EnrollmentRequest>,
    ) -> Result<Response<v1::EnrollmentRequestStatus>, Status> {
        let submission =
            submission_from_wire(request.into_inner()).map_err(Status::invalid_argument)?;
        let status = self
            .store
            .lock()
            .await
            .submit(&submission)
            .await
            .map_err(map_store_error)?;
        Ok(Response::new(status_to_wire(status)))
    }

    async fn get_activation_challenge(
        &self,
        request: Request<v1::EnrollmentChallengeRequest>,
    ) -> Result<Response<v1::EnrollmentChallenge>, Status> {
        let binding = binding_from_challenge_request(request.into_inner())
            .map_err(Status::invalid_argument)?;
        let mut nonce = [0_u8; 32];
        getrandom::getrandom(&mut nonce).map_err(|error| {
            Status::internal(format!("could not generate activation nonce: {error}"))
        })?;
        let challenge = self
            .store
            .lock()
            .await
            .issue_challenge(&binding, &nonce, SystemTime::now())
            .await
            .map_err(map_store_error)?;
        Ok(Response::new(v1::EnrollmentChallenge {
            request_id: challenge.request.request_id,
            tenant_id: challenge.request.tenant_id,
            repository_id: challenge.request.repository_id,
            proposed_node_id: challenge.request.proposed_node_id,
            public_key_fingerprint: challenge.request.public_key_fingerprint,
            nonce: challenge.nonce,
            issued_at: rfc3339(challenge.issued_at).map_err(Status::internal)?,
            expires_at: rfc3339(challenge.expires_at).map_err(Status::internal)?,
            state: state_to_wire(challenge.state),
        }))
    }

    async fn activate_enrollment(
        &self,
        request: Request<v1::EnrollmentActivationProof>,
    ) -> Result<Response<v1::EnrollmentActivationResult>, Status> {
        let proof = request.into_inner();
        let activation = EnrollmentActivation {
            request: binding_from_proof(&proof).map_err(Status::invalid_argument)?,
            nonce: required_bytes(proof.nonce, "nonce").map_err(Status::invalid_argument)?,
            signature: required_bytes(proof.signature, "signature")
                .map_err(Status::invalid_argument)?,
        };
        let receipt_id = new_receipt_id().map_err(Status::internal)?;
        let signing_key_id = new_signing_key_id().map_err(Status::internal)?;
        let result = self
            .store
            .lock()
            .await
            .activate(&activation, &receipt_id, &signing_key_id, SystemTime::now())
            .await
            .map_err(map_store_error)?;
        Ok(Response::new(v1::EnrollmentActivationResult {
            request_id: result.request_id,
            state: state_to_wire(result.state),
            enrolment_receipt_id: result.enrollment_receipt_id,
            rejection_reason: v1::EnrollmentRejectionReason::Unspecified as i32,
            diagnostic: String::new(),
            signing_key_id: result.signing_key_id,
        }))
    }

    async fn rotate_node_key(
        &self,
        request: Request<v1::KeyRotationRequest>,
    ) -> Result<Response<v1::KeyRotationResult>, Status> {
        let rotation =
            key_rotation_from_wire(request.into_inner()).map_err(Status::invalid_argument)?;
        let result = self
            .store
            .lock()
            .await
            .rotate_key(&rotation, SystemTime::now())
            .await
            .map_err(map_store_error)?;
        Ok(Response::new(key_rotation_result_to_wire(result)))
    }

    /// Answers whether a candidate (node, key) binding is enrolled right now.
    /// Always `Ok` (ADR-0122 decision 5): an absent binding, a mismatched
    /// candidate/authentication pair, an invalid signature, a stale
    /// timestamp, and a replayed nonce all collapse to the identical
    /// unverified result. A genuine transport/store failure below still
    /// surfaces as a `Status` -- that is what a caller reads as the arbiter
    /// being unreachable, never as a verified "not enrolled" answer.
    async fn check_enrollment_status(
        &self,
        request: Request<v1::EnrollmentStatusRequest>,
    ) -> Result<Response<v1::EnrollmentStatusResult>, Status> {
        let (
            tenant_id,
            repository_id,
            candidate_node_id,
            candidate_key_fingerprint,
            authentication,
        ) = validated_status_request(request.into_inner()).map_err(Status::invalid_argument)?;

        // The authentication sub-message binds its own node_id/key_fingerprint
        // into the signed bytes (enrollment_status_signing_bytes); requiring
        // them to match the request's candidate fields here, before ever
        // touching the store, keeps "which binding this claims to prove" and
        // "which binding this actually signed for" from ever being able to
        // diverge silently.
        let authentication_names_the_candidate =
            authentication.as_ref().is_some_and(|authentication| {
                authentication.node_id == candidate_node_id
                    && authentication.key_fingerprint == candidate_key_fingerprint
            });
        if !authentication_names_the_candidate {
            return Ok(Response::new(unverified_enrollment_status()));
        }

        let now = SystemTime::now();
        let binding = self
            .store
            .lock()
            .await
            .find_binding(
                &tenant_id,
                &repository_id,
                &candidate_node_id,
                &candidate_key_fingerprint,
                now,
            )
            .await
            .map_err(map_store_error)?;
        let Some(binding) = binding else {
            return Ok(Response::new(unverified_enrollment_status()));
        };

        if enrollment_status_signature::verify(
            &tenant_id,
            &repository_id,
            &binding.public_key,
            authentication.as_ref(),
            now,
        )
        .is_err()
        {
            return Ok(Response::new(unverified_enrollment_status()));
        }

        // Only reached once a genuine signature is confirmed: a forged
        // request must never be able to burn a legitimate nonce out from
        // under its owner.
        let authentication = authentication.expect("checked non-empty above");
        let fresh = self
            .store
            .lock()
            .await
            .consume_status_nonce(
                &tenant_id,
                &repository_id,
                &candidate_node_id,
                &candidate_key_fingerprint,
                &authentication.nonce,
                now,
            )
            .await
            .map_err(map_store_error)?;
        if !fresh {
            return Ok(Response::new(unverified_enrollment_status()));
        }

        Ok(Response::new(v1::EnrollmentStatusResult {
            verified: true,
            state: state_to_wire(binding.state),
        }))
    }
}

mod wire;
use wire::*;

#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey;

    use super::*;

    /// Service-level sabotage suite for `check_enrollment_status` (ADR-0122
    /// decision 10): every collapsed failure produces the byte-identical
    /// `verified: false` result, and only a genuinely verified caller learns
    /// its real lifecycle state -- proven here through the actual gRPC
    /// service, not just its component parts.
    mod check_enrollment_status_tests {
        use ed25519_dalek::Signer;

        use crate::signing_keys::{self, KeyRevocation, SigningKeyRecord};
        use ackplane_protocol::v1::node_enrollment_service_server::NodeEnrollmentService as _;

        use super::*;

        async fn register_test_key(
            database_url: &str,
            tenant_id: &str,
            repository_id: &str,
            node_id: &str,
            fingerprint: &str,
            signing_key: &SigningKey,
        ) {
            let (mut client, connection) =
                tokio_postgres::connect(database_url, tokio_postgres::NoTls)
                    .await
                    .expect("the gated test database should accept a connection");
            tokio::spawn(async move {
                let _ = connection.await;
            });
            let transaction = client
                .transaction()
                .await
                .expect("a transaction should open for key registration");
            signing_keys::register(
                &transaction,
                &SigningKeyRecord {
                    signing_key_id: format!("enrollment-status-{fingerprint}"),
                    tenant_id: tenant_id.to_owned(),
                    repository_id: repository_id.to_owned(),
                    node_id: node_id.to_owned(),
                    public_key: signing_key.verifying_key().to_bytes().to_vec(),
                    public_key_fingerprint: fingerprint.to_owned(),
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

        fn signed_request(
            tenant_id: &str,
            repository_id: &str,
            candidate_node_id: &str,
            candidate_key_fingerprint: &str,
            signing_key: &SigningKey,
            nonce: u8,
        ) -> v1::EnrollmentStatusRequest {
            let mut authentication = v1::EnrollmentStatusAuthentication {
                node_id: candidate_node_id.to_owned(),
                key_fingerprint: candidate_key_fingerprint.to_owned(),
                signed_at: rfc3339(SystemTime::now()).expect("now is representable"),
                nonce: vec![nonce; 16],
                signature: Vec::new(),
            };
            let bytes = ackplane_protocol::enrollment_status_auth::enrollment_status_signing_bytes(
                tenant_id,
                repository_id,
                ackplane_protocol::enrollment_status_auth::EnrollmentStatusOperation::Check,
                &authentication,
            );
            authentication.signature = signing_key.sign(&bytes).to_bytes().to_vec();
            v1::EnrollmentStatusRequest {
                tenant_id: tenant_id.to_owned(),
                repository_id: repository_id.to_owned(),
                candidate_node_id: candidate_node_id.to_owned(),
                candidate_key_fingerprint: candidate_key_fingerprint.to_owned(),
                authentication: Some(authentication),
            }
        }

        #[tokio::test]
        async fn a_verified_query_reports_the_bindings_real_active_state() {
            let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
                println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
                return;
            };
            let unique = crate::test_support::uuid_ish();
            let tenant_id = format!("enrollment-status-tenant-{unique}");
            let repository_id = "repo-a".to_owned();
            let node_id = format!("node-{unique}");
            let fingerprint = format!("fingerprint-{unique}");
            let signing_key = SigningKey::from_bytes(&[unique as u8; 32]);
            register_test_key(
                &database_url,
                &tenant_id,
                &repository_id,
                &node_id,
                &fingerprint,
                &signing_key,
            )
            .await;

            let store = EnrollmentStore::connect(&database_url)
                .await
                .expect("connect enrollment store");
            let service = NodeEnrollmentService::new(store);
            let request = signed_request(
                &tenant_id,
                &repository_id,
                &node_id,
                &fingerprint,
                &signing_key,
                1,
            );
            let response = service
                .check_enrollment_status(Request::new(request))
                .await
                .expect("the RPC itself succeeds")
                .into_inner();

            assert_eq!(
                response,
                v1::EnrollmentStatusResult {
                    verified: true,
                    state: v1::EnrollmentState::Active as i32,
                }
            );
        }

        /// The other authoritative source `find_binding` must consult: a
        /// request that has never been activated has no `signing_keys` row
        /// at all, so its own candidate must still be able to learn its
        /// pending state from `enrollment_requests` -- proving the fallback
        /// path, not just the `signing_keys`-first path every other test in
        /// this module exercises.
        #[tokio::test]
        async fn a_pending_enrollment_reports_its_state_from_the_enrollment_request() {
            let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
                println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
                return;
            };
            let unique = crate::test_support::uuid_ish();
            let tenant_id = format!("enrollment-status-pending-tenant-{unique}");
            let repository_id = "repo-a".to_owned();
            let node_id = format!("node-{unique}");
            let signing_key = SigningKey::from_bytes(&[unique as u8; 32]);
            let fingerprint = public_key_fingerprint(&signing_key.verifying_key().to_bytes());

            let mut submitting_store = EnrollmentStore::connect(&database_url)
                .await
                .expect("connect enrollment store for submission");
            submitting_store
                .submit(&EnrollmentSubmission {
                    request_id: format!("request-{unique}"),
                    tenant_id: tenant_id.clone(),
                    repository_id: repository_id.clone(),
                    proposed_node_id: node_id.clone(),
                    display_name: "Pending node".to_owned(),
                    public_key: signing_key.verifying_key().to_bytes().to_vec(),
                    public_key_fingerprint: fingerprint.clone(),
                    requested_capabilities: vec!["synchronize".to_owned()],
                    created_at: rfc3339(SystemTime::now()).expect("now is representable"),
                    expires_at: rfc3339(SystemTime::now() + std::time::Duration::from_secs(3600))
                        .expect("expiry is representable"),
                })
                .await
                .expect("submitting the pending enrollment request should succeed");

            let store = EnrollmentStore::connect(&database_url)
                .await
                .expect("connect enrollment store for the status check");
            let service = NodeEnrollmentService::new(store);
            let request = signed_request(
                &tenant_id,
                &repository_id,
                &node_id,
                &fingerprint,
                &signing_key,
                1,
            );
            let response = service
                .check_enrollment_status(Request::new(request))
                .await
                .expect("the RPC itself succeeds")
                .into_inner();

            assert_eq!(
                response,
                v1::EnrollmentStatusResult {
                    verified: true,
                    state: v1::EnrollmentState::Pending as i32,
                }
            );
        }

        #[tokio::test]
        async fn a_revoked_binding_still_verifies_and_reports_revoked() {
            let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
                println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
                return;
            };
            let unique = crate::test_support::uuid_ish();
            let tenant_id = format!("enrollment-status-revoked-tenant-{unique}");
            let repository_id = "repo-a".to_owned();
            let node_id = format!("node-{unique}");
            let fingerprint = format!("fingerprint-{unique}");
            let signing_key = SigningKey::from_bytes(&[unique as u8; 32]);
            register_test_key(
                &database_url,
                &tenant_id,
                &repository_id,
                &node_id,
                &fingerprint,
                &signing_key,
            )
            .await;

            let (mut client, connection) =
                tokio_postgres::connect(&database_url, tokio_postgres::NoTls)
                    .await
                    .expect("the gated test database should accept a connection");
            tokio::spawn(async move {
                let _ = connection.await;
            });
            let transaction = client.transaction().await.expect("begin transaction");
            signing_keys::revoke(
                &transaction,
                &KeyRevocation {
                    signing_key_id: format!("enrollment-status-{fingerprint}"),
                    reason: "enrollment status test revocation".to_owned(),
                },
                SystemTime::now(),
            )
            .await
            .expect("revoke the test key");
            transaction.commit().await.expect("commit revocation");

            let store = EnrollmentStore::connect(&database_url)
                .await
                .expect("connect enrollment store");
            let service = NodeEnrollmentService::new(store);
            let request = signed_request(
                &tenant_id,
                &repository_id,
                &node_id,
                &fingerprint,
                &signing_key,
                1,
            );
            let response = service
                .check_enrollment_status(Request::new(request))
                .await
                .expect("the RPC itself succeeds")
                .into_inner();

            // ADR-0122 decision 6: revocation ends new authority, it does not
            // confiscate the key -- a still-key-holding, now-revoked caller
            // may still learn it is revoked.
            assert_eq!(
                response,
                v1::EnrollmentStatusResult {
                    verified: true,
                    state: v1::EnrollmentState::Revoked as i32,
                }
            );
        }

        #[tokio::test]
        async fn a_binding_that_was_never_enrolled_is_reported_as_unverified() {
            let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
                println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
                return;
            };
            let unique = crate::test_support::uuid_ish();
            let signing_key = SigningKey::from_bytes(&[unique as u8; 32]);

            let store = EnrollmentStore::connect(&database_url)
                .await
                .expect("connect enrollment store");
            let service = NodeEnrollmentService::new(store);
            let request = signed_request(
                &format!("enrollment-status-never-tenant-{unique}"),
                "repo-a",
                &format!("node-{unique}"),
                &format!("fingerprint-{unique}"),
                &signing_key,
                1,
            );
            let response = service
                .check_enrollment_status(Request::new(request))
                .await
                .expect("the RPC itself succeeds")
                .into_inner();

            assert_eq!(
                response,
                v1::EnrollmentStatusResult {
                    verified: false,
                    state: v1::EnrollmentState::Unspecified as i32,
                }
            );
        }

        #[tokio::test]
        async fn a_mismatched_candidate_key_fingerprint_is_reported_as_unverified() {
            let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
                println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
                return;
            };
            let unique = crate::test_support::uuid_ish();
            let tenant_id = format!("enrollment-status-mismatch-tenant-{unique}");
            let repository_id = "repo-a".to_owned();
            let node_id = format!("node-{unique}");
            let fingerprint = format!("fingerprint-{unique}");
            let signing_key = SigningKey::from_bytes(&[unique as u8; 32]);
            register_test_key(
                &database_url,
                &tenant_id,
                &repository_id,
                &node_id,
                &fingerprint,
                &signing_key,
            )
            .await;

            let store = EnrollmentStore::connect(&database_url)
                .await
                .expect("connect enrollment store");
            let service = NodeEnrollmentService::new(store);
            // A different fingerprint than the one actually on file: the
            // authentication is internally consistent (it names the same
            // fingerprint it claims), but it does not match what is
            // registered, so the lookup itself must fail to find a binding.
            let wrong_fingerprint = format!("wrong-{fingerprint}");
            let request = signed_request(
                &tenant_id,
                &repository_id,
                &node_id,
                &wrong_fingerprint,
                &signing_key,
                1,
            );
            let response = service
                .check_enrollment_status(Request::new(request))
                .await
                .expect("the RPC itself succeeds")
                .into_inner();

            assert_eq!(
                response,
                v1::EnrollmentStatusResult {
                    verified: false,
                    state: v1::EnrollmentState::Unspecified as i32,
                }
            );
        }

        #[tokio::test]
        async fn a_replayed_authentication_is_refused_on_its_second_use() {
            let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
                println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
                return;
            };
            let unique = crate::test_support::uuid_ish();
            let tenant_id = format!("enrollment-status-replay-tenant-{unique}");
            let repository_id = "repo-a".to_owned();
            let node_id = format!("node-{unique}");
            let fingerprint = format!("fingerprint-{unique}");
            let signing_key = SigningKey::from_bytes(&[unique as u8; 32]);
            register_test_key(
                &database_url,
                &tenant_id,
                &repository_id,
                &node_id,
                &fingerprint,
                &signing_key,
            )
            .await;

            let store = EnrollmentStore::connect(&database_url)
                .await
                .expect("connect enrollment store");
            let service = NodeEnrollmentService::new(store);
            let request = signed_request(
                &tenant_id,
                &repository_id,
                &node_id,
                &fingerprint,
                &signing_key,
                1,
            );

            let first = service
                .check_enrollment_status(Request::new(request.clone()))
                .await
                .expect("the first RPC succeeds")
                .into_inner();
            assert!(first.verified, "the first use must verify");

            let second = service
                .check_enrollment_status(Request::new(request))
                .await
                .expect("the replayed RPC still succeeds at the transport level")
                .into_inner();
            assert_eq!(
                second,
                v1::EnrollmentStatusResult {
                    verified: false,
                    state: v1::EnrollmentState::Unspecified as i32,
                },
                "an identical, already-consumed authentication must not verify twice"
            );
        }

        #[tokio::test]
        async fn a_signature_produced_for_a_different_domain_does_not_verify() {
            let Ok(database_url) = std::env::var("ACKPLANE_TEST_DATABASE_URL") else {
                println!("skipped: ACKPLANE_TEST_DATABASE_URL not set");
                return;
            };
            let unique = crate::test_support::uuid_ish();
            let tenant_id = format!("enrollment-status-domain-tenant-{unique}");
            let repository_id = "repo-a".to_owned();
            let node_id = format!("node-{unique}");
            let fingerprint = format!("fingerprint-{unique}");
            let signing_key = SigningKey::from_bytes(&[unique as u8; 32]);
            register_test_key(
                &database_url,
                &tenant_id,
                &repository_id,
                &node_id,
                &fingerprint,
                &signing_key,
            )
            .await;

            // A signature over the identical fields, but produced under
            // claim_auth's domain separator rather than enrollment_status's
            // own -- proving the two domains can never be confused for one
            // another even when every other field coincides.
            let mut authentication = v1::EnrollmentStatusAuthentication {
                node_id: node_id.clone(),
                key_fingerprint: fingerprint.clone(),
                signed_at: rfc3339(SystemTime::now()).expect("now is representable"),
                nonce: vec![9; 16],
                signature: Vec::new(),
            };
            let mut foreign_domain_bytes = ackplane_protocol::claim_auth::CLAIM_DOMAIN.to_vec();
            foreign_domain_bytes.extend_from_slice(tenant_id.as_bytes());
            foreign_domain_bytes.extend_from_slice(repository_id.as_bytes());
            foreign_domain_bytes.extend_from_slice(node_id.as_bytes());
            foreign_domain_bytes.extend_from_slice(fingerprint.as_bytes());
            authentication.signature = signing_key.sign(&foreign_domain_bytes).to_bytes().to_vec();

            let store = EnrollmentStore::connect(&database_url)
                .await
                .expect("connect enrollment store");
            let service = NodeEnrollmentService::new(store);
            let request = v1::EnrollmentStatusRequest {
                tenant_id,
                repository_id,
                candidate_node_id: node_id,
                candidate_key_fingerprint: fingerprint,
                authentication: Some(authentication),
            };
            let response = service
                .check_enrollment_status(Request::new(request))
                .await
                .expect("the RPC itself succeeds")
                .into_inner();

            assert_eq!(
                response,
                v1::EnrollmentStatusResult {
                    verified: false,
                    state: v1::EnrollmentState::Unspecified as i32,
                }
            );
        }
    }
}

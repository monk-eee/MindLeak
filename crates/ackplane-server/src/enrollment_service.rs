//! gRPC transport for Ackplane's node enrollment authority (ADR-0085).

use std::{sync::Arc, time::SystemTime};

use ackplane_protocol::v1;
use ed25519_dalek::VerifyingKey;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

use crate::{
    enrollment::{public_key_fingerprint, EnrollmentState},
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
}

fn submission_from_wire(request: v1::EnrollmentRequest) -> Result<EnrollmentSubmission, String> {
    let public_key = required_bytes(request.public_key, "public_key")?;
    let public_key_array = <&[u8; 32]>::try_from(public_key.as_slice())
        .map_err(|_| "public_key must be an Ed25519 public key".to_owned())?;
    VerifyingKey::from_bytes(public_key_array)
        .map_err(|_| "public_key must be an Ed25519 public key".to_owned())?;
    let declared_fingerprint = required(request.public_key_fingerprint, "public_key_fingerprint")?;
    if public_key_fingerprint(&public_key) != declared_fingerprint {
        return Err("public_key_fingerprint does not match public_key".to_owned());
    }

    Ok(EnrollmentSubmission {
        request_id: required(request.request_id, "request_id")?,
        tenant_id: required(request.tenant_id, "tenant_id")?,
        repository_id: required(request.repository_id, "repository_id")?,
        proposed_node_id: required(request.proposed_node_id, "proposed_node_id")?,
        display_name: required(request.display_name, "display_name")?,
        public_key,
        public_key_fingerprint: declared_fingerprint,
        requested_capabilities: request.requested_capabilities,
        created_at: required(request.created_at, "created_at")?,
        expires_at: required(request.expires_at, "expires_at")?,
    })
}

fn binding_from_challenge_request(
    request: v1::EnrollmentChallengeRequest,
) -> Result<ActivationChallengeRequest, String> {
    Ok(ActivationChallengeRequest {
        request_id: required(request.request_id, "request_id")?,
        tenant_id: required(request.tenant_id, "tenant_id")?,
        repository_id: required(request.repository_id, "repository_id")?,
        proposed_node_id: required(request.proposed_node_id, "proposed_node_id")?,
        public_key_fingerprint: required(request.public_key_fingerprint, "public_key_fingerprint")?,
    })
}

fn binding_from_proof(
    proof: &v1::EnrollmentActivationProof,
) -> Result<ActivationChallengeRequest, String> {
    Ok(ActivationChallengeRequest {
        request_id: required(proof.request_id.clone(), "request_id")?,
        tenant_id: required(proof.tenant_id.clone(), "tenant_id")?,
        repository_id: required(proof.repository_id.clone(), "repository_id")?,
        proposed_node_id: required(proof.proposed_node_id.clone(), "proposed_node_id")?,
        public_key_fingerprint: required(
            proof.public_key_fingerprint.clone(),
            "public_key_fingerprint",
        )?,
    })
}

fn key_rotation_from_wire(request: v1::KeyRotationRequest) -> Result<KeyRotation, String> {
    Ok(KeyRotation {
        tenant_id: required(request.tenant_id, "tenant_id")?,
        repository_id: required(request.repository_id, "repository_id")?,
        node_id: required(request.node_id, "node_id")?,
        current_key_id: required(request.current_key_id, "current_key_id")?,
        successor_key_id: required(request.successor_key_id, "successor_key_id")?,
        successor_public_key_fingerprint: required(
            request.successor_public_key_fingerprint,
            "successor_public_key_fingerprint",
        )?,
        successor_public_key: required_bytes(request.successor_public_key, "successor_public_key")?,
        current_key_signature: required_bytes(
            request.current_key_signature,
            "current_key_signature",
        )?,
        successor_key_signature: required_bytes(
            request.successor_key_signature,
            "successor_key_signature",
        )?,
        requested_overlap_seconds: request.requested_overlap_seconds,
    })
}

fn key_rotation_result_to_wire(
    result: crate::enrollment_store::KeyRotationResult,
) -> v1::KeyRotationResult {
    let (outcome, rejection_reason, diagnostic) = match result.outcome {
        KeyRotationOutcome::Applied => (
            v1::KeyRotationOutcome::Applied,
            v1::KeyRotationRejectionReason::Unspecified,
            String::new(),
        ),
        KeyRotationOutcome::Rejected(reason) => (
            v1::KeyRotationOutcome::Rejected,
            key_rotation_rejection_to_wire(reason),
            key_rotation_rejection_diagnostic(reason).to_owned(),
        ),
    };
    v1::KeyRotationResult {
        node_id: result.node_id,
        current_key_id: result.current_key_id,
        successor_key_id: result.successor_key_id,
        outcome: outcome as i32,
        rejection_reason: rejection_reason as i32,
        diagnostic,
    }
}

fn key_rotation_rejection_to_wire(reason: KeyRotationRejection) -> v1::KeyRotationRejectionReason {
    match reason {
        KeyRotationRejection::CurrentKeyNotActive => {
            v1::KeyRotationRejectionReason::CurrentKeyNotActive
        }
        KeyRotationRejection::SuccessorKeyConflict => {
            v1::KeyRotationRejectionReason::SuccessorKeyConflict
        }
        KeyRotationRejection::ContinuityProofInvalid => {
            v1::KeyRotationRejectionReason::ContinuityProofInvalid
        }
        KeyRotationRejection::NodeRevoked => v1::KeyRotationRejectionReason::NodeRevoked,
    }
}

fn key_rotation_rejection_diagnostic(reason: KeyRotationRejection) -> &'static str {
    match reason {
        KeyRotationRejection::CurrentKeyNotActive => {
            "the current key is not an active signing key for this node"
        }
        KeyRotationRejection::SuccessorKeyConflict => "the successor key id is already registered",
        KeyRotationRejection::ContinuityProofInvalid => {
            "the continuity proof does not verify against both the current and successor keys"
        }
        KeyRotationRejection::NodeRevoked => "the current key has been revoked",
    }
}

fn required(value: String, field: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        Err(format!("{field} is required"))
    } else {
        Ok(value)
    }
}

fn required_bytes(value: Vec<u8>, field: &str) -> Result<Vec<u8>, String> {
    if value.is_empty() {
        Err(format!("{field} is required"))
    } else {
        Ok(value)
    }
}

fn state_to_wire(state: EnrollmentState) -> i32 {
    match state {
        EnrollmentState::Pending => v1::EnrollmentState::Pending as i32,
        EnrollmentState::Approved => v1::EnrollmentState::Approved as i32,
        EnrollmentState::Activating => v1::EnrollmentState::Activating as i32,
        EnrollmentState::Active => v1::EnrollmentState::Active as i32,
        EnrollmentState::Expired => v1::EnrollmentState::Expired as i32,
        EnrollmentState::Rejected => v1::EnrollmentState::Rejected as i32,
        EnrollmentState::Revoked => v1::EnrollmentState::Revoked as i32,
    }
}

fn status_to_wire(status: EnrollmentStatus) -> v1::EnrollmentRequestStatus {
    v1::EnrollmentRequestStatus {
        request_id: status.request_id,
        state: state_to_wire(status.state),
        rejection_reason: v1::EnrollmentRejectionReason::Unspecified as i32,
        diagnostic: String::new(),
    }
}

fn rfc3339(timestamp: SystemTime) -> Result<String, String> {
    OffsetDateTime::from(timestamp)
        .format(&Rfc3339)
        .map_err(|error| format!("could not format timestamp: {error}"))
}

fn new_receipt_id() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| format!("could not generate enrollment receipt id: {error}"))?;
    let encoded = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("enrollment-{encoded}"))
}

/// Opaque, and deliberately not derived from the key or its fingerprint: this
/// names one binding of a key to a node for one lifetime, so re-enrolling the
/// same key material must produce a different id rather than collide with the
/// history of the earlier binding.
fn new_signing_key_id() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| format!("could not generate signing key id: {error}"))?;
    let encoded = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("signing-key-{encoded}"))
}

fn map_store_error(error: EnrollmentStoreError) -> Status {
    match error {
        EnrollmentStoreError::RequestConflict { .. } => Status::already_exists(error.to_string()),
        EnrollmentStoreError::PublicKeyFingerprintMismatch { .. } => {
            Status::invalid_argument(error.to_string())
        }
        EnrollmentStoreError::InvalidTimestamp { .. } => {
            Status::invalid_argument(error.to_string())
        }
        EnrollmentStoreError::NotFound { .. } => Status::not_found(error.to_string()),
        EnrollmentStoreError::BindingMismatch { .. }
        | EnrollmentStoreError::FingerprintMismatch { .. }
        | EnrollmentStoreError::InvalidProof { .. } => Status::permission_denied(error.to_string()),
        EnrollmentStoreError::NotPending { .. }
        | EnrollmentStoreError::NotApproved { .. }
        | EnrollmentStoreError::RequestExpired { .. }
        | EnrollmentStoreError::ChallengeExpired { .. }
        | EnrollmentStoreError::ChallengeConsumed { .. } => {
            Status::failed_precondition(error.to_string())
        }
        EnrollmentStoreError::Database(_)
        | EnrollmentStoreError::SigningKey(_)
        | EnrollmentStoreError::UnknownState { .. } => Status::internal(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use ed25519_dalek::SigningKey;
    use tonic::Code;

    use super::*;

    fn enrollment_request(public_key: Vec<u8>) -> v1::EnrollmentRequest {
        v1::EnrollmentRequest {
            request_id: "request-1".to_owned(),
            tenant_id: "tenant-1".to_owned(),
            repository_id: "repository-1".to_owned(),
            proposed_node_id: "node-1".to_owned(),
            display_name: "Node one".to_owned(),
            public_key_fingerprint: "fingerprint-1".to_owned(),
            requested_capabilities: vec!["synchronize".to_owned()],
            created_at: "2026-08-14T00:00:00Z".to_owned(),
            expires_at: "2026-08-15T00:00:00Z".to_owned(),
            public_key,
        }
    }

    #[test]
    fn submission_rejects_a_public_key_that_is_not_ed25519_sized() {
        let error = submission_from_wire(enrollment_request(vec![1; 31])).unwrap_err();

        assert_eq!(error, "public_key must be an Ed25519 public key");
    }

    #[test]
    fn submission_preserves_a_valid_public_key_and_its_explicit_binding() {
        let signing_key = SigningKey::from_bytes(&[7; 32]);
        let mut request = enrollment_request(signing_key.verifying_key().to_bytes().to_vec());
        let fingerprint = public_key_fingerprint(&signing_key.verifying_key().to_bytes());
        request.public_key_fingerprint = fingerprint.clone();
        let submission = submission_from_wire(request).expect("valid Ed25519 key is accepted");

        assert_eq!(
            submission,
            EnrollmentSubmission {
                request_id: "request-1".to_owned(),
                tenant_id: "tenant-1".to_owned(),
                repository_id: "repository-1".to_owned(),
                proposed_node_id: "node-1".to_owned(),
                display_name: "Node one".to_owned(),
                public_key: signing_key.verifying_key().to_bytes().to_vec(),
                public_key_fingerprint: fingerprint,
                requested_capabilities: vec!["synchronize".to_owned()],
                created_at: "2026-08-14T00:00:00Z".to_owned(),
                expires_at: "2026-08-15T00:00:00Z".to_owned(),
            }
        );
    }

    #[test]
    fn submission_rejects_a_fingerprint_for_a_different_public_key() {
        let signing_key = SigningKey::from_bytes(&[7; 32]);
        let mut request = enrollment_request(signing_key.verifying_key().to_bytes().to_vec());
        request.public_key_fingerprint = public_key_fingerprint(&[8; 32]);

        assert_eq!(
            submission_from_wire(request),
            Err("public_key_fingerprint does not match public_key".to_owned())
        );
    }

    #[test]
    fn invalid_proof_maps_to_a_permission_denied_response() {
        let status = map_store_error(EnrollmentStoreError::InvalidProof {
            request_id: "request-1".to_owned(),
        });

        assert_eq!(
            (status.code(), status.message()),
            (
                Code::PermissionDenied,
                "activation proof for enrollment request request-1 is invalid"
            )
        );
    }

    #[test]
    fn activation_timestamps_are_rfc3339() {
        let timestamp = rfc3339(SystemTime::UNIX_EPOCH).expect("Unix epoch is representable");

        assert_eq!(timestamp, "1970-01-01T00:00:00Z");
    }
}

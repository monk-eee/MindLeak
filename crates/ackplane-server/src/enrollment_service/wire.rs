use super::*;

pub(super) fn submission_from_wire(
    request: v1::EnrollmentRequest,
) -> Result<EnrollmentSubmission, String> {
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

pub(super) fn binding_from_challenge_request(
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

pub(super) fn binding_from_proof(
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

pub(super) fn key_rotation_from_wire(
    request: v1::KeyRotationRequest,
) -> Result<KeyRotation, String> {
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

/// Extract and validate the structural fields of a `CheckEnrollmentStatus`
/// request. `authentication` is deliberately left as `Option` rather than
/// required here: an absent authentication is a verification failure, not a
/// malformed request, and per ADR-0122 decision 5 it must collapse into the
/// same unverified result every other verification failure does -- not a
/// distinguishable `Status::invalid_argument`.
pub(super) fn validated_status_request(
    request: v1::EnrollmentStatusRequest,
) -> Result<
    (
        String,
        String,
        String,
        String,
        Option<v1::EnrollmentStatusAuthentication>,
    ),
    String,
> {
    Ok((
        required(request.tenant_id, "tenant_id")?,
        required(request.repository_id, "repository_id")?,
        required(request.candidate_node_id, "candidate_node_id")?,
        required(
            request.candidate_key_fingerprint,
            "candidate_key_fingerprint",
        )?,
        request.authentication,
    ))
}

/// The one shape every `CheckEnrollmentStatus` verification failure produces
/// (ADR-0122 decision 5) -- an absent binding, a mismatched candidate, an
/// invalid signature, a stale timestamp, and a replayed nonce are all
/// indistinguishable from each other and from "this node has never enrolled".
pub(super) fn unverified_enrollment_status() -> v1::EnrollmentStatusResult {
    v1::EnrollmentStatusResult {
        verified: false,
        state: v1::EnrollmentState::Unspecified as i32,
    }
}

pub(super) fn key_rotation_result_to_wire(
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

pub(super) fn required(value: String, field: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        Err(format!("{field} is required"))
    } else {
        Ok(value)
    }
}

pub(super) fn required_bytes(value: Vec<u8>, field: &str) -> Result<Vec<u8>, String> {
    if value.is_empty() {
        Err(format!("{field} is required"))
    } else {
        Ok(value)
    }
}

pub(super) fn state_to_wire(state: EnrollmentState) -> i32 {
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

pub(super) fn status_to_wire(status: EnrollmentStatus) -> v1::EnrollmentRequestStatus {
    v1::EnrollmentRequestStatus {
        request_id: status.request_id,
        state: state_to_wire(status.state),
        rejection_reason: v1::EnrollmentRejectionReason::Unspecified as i32,
        diagnostic: String::new(),
    }
}

pub(super) fn rfc3339(timestamp: SystemTime) -> Result<String, String> {
    crate::wire_format::rfc3339(timestamp)
        .map_err(|error| format!("could not format timestamp: {error}"))
}

pub(super) fn new_receipt_id() -> Result<String, String> {
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
pub(super) fn new_signing_key_id() -> Result<String, String> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| format!("could not generate signing key id: {error}"))?;
    let encoded = bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!("signing-key-{encoded}"))
}

pub(super) fn map_store_error(error: EnrollmentStoreError) -> Status {
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
    // Generated by AI (UnitTest MCP)
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

    #[test]
    fn required_rejects_an_empty_or_whitespace_only_string() {
        assert_eq!(
            required(String::new(), "tenant_id"),
            Err("tenant_id is required".to_owned())
        );
        assert_eq!(
            required("   ".to_owned(), "tenant_id"),
            Err("tenant_id is required".to_owned())
        );
        assert_eq!(
            required("node-1".to_owned(), "tenant_id"),
            Ok("node-1".to_owned())
        );
    }

    #[test]
    fn required_bytes_rejects_only_an_empty_vec() {
        assert_eq!(
            required_bytes(Vec::new(), "public_key"),
            Err("public_key is required".to_owned())
        );
        assert_eq!(required_bytes(vec![0], "public_key"), Ok(vec![0]));
    }

    #[test]
    fn binding_from_challenge_request_carries_every_field_through() {
        let request = v1::EnrollmentChallengeRequest {
            request_id: "request-1".to_owned(),
            tenant_id: "tenant-1".to_owned(),
            repository_id: "repository-1".to_owned(),
            proposed_node_id: "node-1".to_owned(),
            public_key_fingerprint: "fingerprint-1".to_owned(),
        };

        assert_eq!(
            binding_from_challenge_request(request),
            Ok(ActivationChallengeRequest {
                request_id: "request-1".to_owned(),
                tenant_id: "tenant-1".to_owned(),
                repository_id: "repository-1".to_owned(),
                proposed_node_id: "node-1".to_owned(),
                public_key_fingerprint: "fingerprint-1".to_owned(),
            })
        );
    }

    #[test]
    fn binding_from_challenge_request_reports_the_first_missing_field() {
        let request = v1::EnrollmentChallengeRequest {
            request_id: String::new(),
            tenant_id: "tenant-1".to_owned(),
            repository_id: "repository-1".to_owned(),
            proposed_node_id: "node-1".to_owned(),
            public_key_fingerprint: "fingerprint-1".to_owned(),
        };

        assert_eq!(
            binding_from_challenge_request(request),
            Err("request_id is required".to_owned())
        );
    }

    #[test]
    fn binding_from_proof_carries_every_field_through() {
        let proof = v1::EnrollmentActivationProof {
            request_id: "request-1".to_owned(),
            tenant_id: "tenant-1".to_owned(),
            repository_id: "repository-1".to_owned(),
            proposed_node_id: "node-1".to_owned(),
            public_key_fingerprint: "fingerprint-1".to_owned(),
            nonce: vec![1, 2, 3],
            signature: vec![4, 5, 6],
        };

        assert_eq!(
            binding_from_proof(&proof),
            Ok(ActivationChallengeRequest {
                request_id: "request-1".to_owned(),
                tenant_id: "tenant-1".to_owned(),
                repository_id: "repository-1".to_owned(),
                proposed_node_id: "node-1".to_owned(),
                public_key_fingerprint: "fingerprint-1".to_owned(),
            })
        );
    }

    #[test]
    fn binding_from_proof_reports_a_missing_fingerprint() {
        let proof = v1::EnrollmentActivationProof {
            request_id: "request-1".to_owned(),
            tenant_id: "tenant-1".to_owned(),
            repository_id: "repository-1".to_owned(),
            proposed_node_id: "node-1".to_owned(),
            public_key_fingerprint: String::new(),
            nonce: vec![1, 2, 3],
            signature: vec![4, 5, 6],
        };

        assert_eq!(
            binding_from_proof(&proof),
            Err("public_key_fingerprint is required".to_owned())
        );
    }

    fn key_rotation_request() -> v1::KeyRotationRequest {
        v1::KeyRotationRequest {
            tenant_id: "tenant-1".to_owned(),
            repository_id: "repository-1".to_owned(),
            node_id: "node-1".to_owned(),
            current_key_id: "signing-key-1".to_owned(),
            successor_key_id: "signing-key-2".to_owned(),
            successor_public_key_fingerprint: "fingerprint-2".to_owned(),
            successor_public_key: vec![1; 32],
            current_key_signature: vec![2; 64],
            successor_key_signature: vec![3; 64],
            requested_overlap_seconds: 3600,
        }
    }

    #[test]
    fn key_rotation_from_wire_carries_every_field_through() {
        assert_eq!(
            key_rotation_from_wire(key_rotation_request()),
            Ok(KeyRotation {
                tenant_id: "tenant-1".to_owned(),
                repository_id: "repository-1".to_owned(),
                node_id: "node-1".to_owned(),
                current_key_id: "signing-key-1".to_owned(),
                successor_key_id: "signing-key-2".to_owned(),
                successor_public_key_fingerprint: "fingerprint-2".to_owned(),
                successor_public_key: vec![1; 32],
                current_key_signature: vec![2; 64],
                successor_key_signature: vec![3; 64],
                requested_overlap_seconds: 3600,
            })
        );
    }

    #[test]
    fn key_rotation_from_wire_rejects_an_empty_successor_public_key() {
        let mut request = key_rotation_request();
        request.successor_public_key = Vec::new();

        assert_eq!(
            key_rotation_from_wire(request),
            Err("successor_public_key is required".to_owned())
        );
    }

    #[test]
    fn validated_status_request_carries_the_binding_and_optional_authentication() {
        let authentication = v1::EnrollmentStatusAuthentication {
            node_id: "node-1".to_owned(),
            key_fingerprint: "fingerprint-1".to_owned(),
            signed_at: "2026-08-14T00:00:00Z".to_owned(),
            nonce: vec![1, 2, 3],
            signature: vec![4, 5, 6],
        };
        let request = v1::EnrollmentStatusRequest {
            tenant_id: "tenant-1".to_owned(),
            repository_id: "repository-1".to_owned(),
            candidate_node_id: "node-1".to_owned(),
            candidate_key_fingerprint: "fingerprint-1".to_owned(),
            authentication: Some(authentication.clone()),
        };

        assert_eq!(
            validated_status_request(request),
            Ok((
                "tenant-1".to_owned(),
                "repository-1".to_owned(),
                "node-1".to_owned(),
                "fingerprint-1".to_owned(),
                Some(authentication),
            ))
        );
    }

    #[test]
    fn validated_status_request_accepts_an_absent_authentication() {
        let request = v1::EnrollmentStatusRequest {
            tenant_id: "tenant-1".to_owned(),
            repository_id: "repository-1".to_owned(),
            candidate_node_id: "node-1".to_owned(),
            candidate_key_fingerprint: "fingerprint-1".to_owned(),
            authentication: None,
        };

        let (.., authentication) = validated_status_request(request).expect("valid request");

        assert_eq!(authentication, None);
    }

    #[test]
    fn validated_status_request_reports_a_missing_candidate_key_fingerprint() {
        let request = v1::EnrollmentStatusRequest {
            tenant_id: "tenant-1".to_owned(),
            repository_id: "repository-1".to_owned(),
            candidate_node_id: "node-1".to_owned(),
            candidate_key_fingerprint: String::new(),
            authentication: None,
        };

        assert_eq!(
            validated_status_request(request),
            Err("candidate_key_fingerprint is required".to_owned())
        );
    }

    #[test]
    fn unverified_enrollment_status_is_unverified_with_no_state() {
        let result = unverified_enrollment_status();

        assert_eq!(
            result,
            v1::EnrollmentStatusResult {
                verified: false,
                state: v1::EnrollmentState::Unspecified as i32,
            }
        );
    }

    #[test]
    fn key_rotation_result_to_wire_reports_an_applied_outcome() {
        let result = crate::enrollment_store::KeyRotationResult {
            node_id: "node-1".to_owned(),
            current_key_id: "signing-key-1".to_owned(),
            successor_key_id: "signing-key-2".to_owned(),
            outcome: KeyRotationOutcome::Applied,
        };

        assert_eq!(
            key_rotation_result_to_wire(result),
            v1::KeyRotationResult {
                node_id: "node-1".to_owned(),
                current_key_id: "signing-key-1".to_owned(),
                successor_key_id: "signing-key-2".to_owned(),
                outcome: v1::KeyRotationOutcome::Applied as i32,
                rejection_reason: v1::KeyRotationRejectionReason::Unspecified as i32,
                diagnostic: String::new(),
            }
        );
    }

    #[test]
    fn key_rotation_result_to_wire_reports_every_rejection_reason_and_its_diagnostic() {
        let cases = [
            (
                KeyRotationRejection::CurrentKeyNotActive,
                v1::KeyRotationRejectionReason::CurrentKeyNotActive,
                "the current key is not an active signing key for this node",
            ),
            (
                KeyRotationRejection::SuccessorKeyConflict,
                v1::KeyRotationRejectionReason::SuccessorKeyConflict,
                "the successor key id is already registered",
            ),
            (
                KeyRotationRejection::ContinuityProofInvalid,
                v1::KeyRotationRejectionReason::ContinuityProofInvalid,
                "the continuity proof does not verify against both the current and successor keys",
            ),
            (
                KeyRotationRejection::NodeRevoked,
                v1::KeyRotationRejectionReason::NodeRevoked,
                "the current key has been revoked",
            ),
        ];

        for (reason, expected_wire_reason, expected_diagnostic) in cases {
            let result = crate::enrollment_store::KeyRotationResult {
                node_id: "node-1".to_owned(),
                current_key_id: "signing-key-1".to_owned(),
                successor_key_id: "signing-key-2".to_owned(),
                outcome: KeyRotationOutcome::Rejected(reason),
            };

            assert_eq!(
                key_rotation_result_to_wire(result),
                v1::KeyRotationResult {
                    node_id: "node-1".to_owned(),
                    current_key_id: "signing-key-1".to_owned(),
                    successor_key_id: "signing-key-2".to_owned(),
                    outcome: v1::KeyRotationOutcome::Rejected as i32,
                    rejection_reason: expected_wire_reason as i32,
                    diagnostic: expected_diagnostic.to_owned(),
                },
                "outcome for {reason:?}"
            );
        }
    }

    #[test]
    fn state_to_wire_maps_every_enrollment_state() {
        let cases = [
            (EnrollmentState::Pending, v1::EnrollmentState::Pending),
            (EnrollmentState::Approved, v1::EnrollmentState::Approved),
            (EnrollmentState::Activating, v1::EnrollmentState::Activating),
            (EnrollmentState::Active, v1::EnrollmentState::Active),
            (EnrollmentState::Expired, v1::EnrollmentState::Expired),
            (EnrollmentState::Rejected, v1::EnrollmentState::Rejected),
            (EnrollmentState::Revoked, v1::EnrollmentState::Revoked),
        ];

        for (state, expected) in cases {
            assert_eq!(state_to_wire(state), expected as i32, "state {state:?}");
        }
    }

    #[test]
    fn status_to_wire_carries_the_request_id_and_mapped_state_with_no_rejection() {
        let status = EnrollmentStatus {
            request_id: "request-1".to_owned(),
            state: EnrollmentState::Approved,
        };

        assert_eq!(
            status_to_wire(status),
            v1::EnrollmentRequestStatus {
                request_id: "request-1".to_owned(),
                state: v1::EnrollmentState::Approved as i32,
                rejection_reason: v1::EnrollmentRejectionReason::Unspecified as i32,
                diagnostic: String::new(),
            }
        );
    }

    #[test]
    fn new_receipt_id_and_new_signing_key_id_are_prefixed_32_hex_chars_and_distinct() {
        let first_receipt = new_receipt_id().expect("randomness is available");
        let second_receipt = new_receipt_id().expect("randomness is available");
        let signing_key_id = new_signing_key_id().expect("randomness is available");

        assert!(first_receipt.starts_with("enrollment-"));
        assert_eq!(first_receipt.trim_start_matches("enrollment-").len(), 32);
        assert_ne!(first_receipt, second_receipt);

        assert!(signing_key_id.starts_with("signing-key-"));
        assert_eq!(signing_key_id.trim_start_matches("signing-key-").len(), 32);
    }

    #[test]
    fn map_store_error_reports_every_error_by_its_grpc_status_code() {
        let cases = [
            (
                EnrollmentStoreError::RequestConflict {
                    request_id: "request-1".to_owned(),
                },
                Code::AlreadyExists,
            ),
            (
                EnrollmentStoreError::PublicKeyFingerprintMismatch {
                    request_id: "request-1".to_owned(),
                },
                Code::InvalidArgument,
            ),
            (
                EnrollmentStoreError::InvalidTimestamp {
                    request_id: "request-1".to_owned(),
                    field: "created_at",
                    value: "not-a-timestamp".to_owned(),
                },
                Code::InvalidArgument,
            ),
            (
                EnrollmentStoreError::NotFound {
                    request_id: "request-1".to_owned(),
                },
                Code::NotFound,
            ),
            (
                EnrollmentStoreError::BindingMismatch {
                    request_id: "request-1".to_owned(),
                },
                Code::PermissionDenied,
            ),
            (
                EnrollmentStoreError::FingerprintMismatch {
                    request_id: "request-1".to_owned(),
                },
                Code::PermissionDenied,
            ),
            (
                EnrollmentStoreError::NotPending {
                    request_id: "request-1".to_owned(),
                },
                Code::FailedPrecondition,
            ),
            (
                EnrollmentStoreError::NotApproved {
                    request_id: "request-1".to_owned(),
                },
                Code::FailedPrecondition,
            ),
            (
                EnrollmentStoreError::RequestExpired {
                    request_id: "request-1".to_owned(),
                },
                Code::FailedPrecondition,
            ),
            (
                EnrollmentStoreError::ChallengeExpired {
                    request_id: "request-1".to_owned(),
                },
                Code::FailedPrecondition,
            ),
            (
                EnrollmentStoreError::ChallengeConsumed {
                    request_id: "request-1".to_owned(),
                },
                Code::FailedPrecondition,
            ),
            (
                EnrollmentStoreError::UnknownState { value: 99 },
                Code::Internal,
            ),
        ];

        for (error, expected_code) in cases {
            let expected_message = error.to_string();
            let status = map_store_error(error);

            assert_eq!(
                (status.code(), status.message()),
                (expected_code, expected_message.as_str())
            );
        }
    }
}

//! Pure enrollment decisions for the Ackplane service (ADR-0085).
//!
//! Storage records these values and the gRPC service maps requests onto them;
//! neither concern decides whether a proof may activate a node.

use std::time::SystemTime;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

const ACTIVATION_DOMAIN: &[u8] = b"mindleak.ackplane.v1.enrollment.activation\0";

/// Return the canonical, human-comparable fingerprint of an Ed25519 public key.
pub fn public_key_fingerprint(public_key: &[u8]) -> String {
    let digest = Sha256::digest(public_key);
    let encoded = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("ed25519:{encoded}")
}

/// Encode the exact domain-separated bytes a node signs to prove possession of
/// its approved key. Every binding is length-delimited so adjacent fields can
/// never be reinterpreted as a different tuple.
pub fn activation_challenge_bytes(
    nonce: &[u8],
    request_id: &str,
    tenant_id: &str,
    repository_id: &str,
    node_id: &str,
    public_key_fingerprint: &str,
) -> Vec<u8> {
    let fields = [
        nonce,
        request_id.as_bytes(),
        tenant_id.as_bytes(),
        repository_id.as_bytes(),
        node_id.as_bytes(),
        public_key_fingerprint.as_bytes(),
    ];
    let mut bytes = Vec::with_capacity(
        ACTIVATION_DOMAIN.len() + fields.iter().map(|field| 4 + field.len()).sum::<usize>(),
    );
    bytes.extend_from_slice(ACTIVATION_DOMAIN);
    for field in fields {
        bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
        bytes.extend_from_slice(field);
    }
    bytes
}

/// The immutable values the activation proof binds together.
pub struct ActivationProofBinding<'a> {
    pub nonce: &'a [u8],
    pub request_id: &'a str,
    pub tenant_id: &'a str,
    pub repository_id: &'a str,
    pub node_id: &'a str,
    pub public_key_fingerprint: &'a str,
}

/// Verify possession of the exact public key approved for the enrollment.
pub fn verify_activation_proof(
    public_key: &[u8],
    signature: &[u8],
    binding: ActivationProofBinding<'_>,
) -> bool {
    let Ok(public_key) = <&[u8; 32]>::try_from(public_key) else {
        return false;
    };
    let Ok(verifying_key) = VerifyingKey::from_bytes(public_key) else {
        return false;
    };
    let Ok(signature) = Signature::from_slice(signature) else {
        return false;
    };
    verifying_key
        .verify(
            &activation_challenge_bytes(
                binding.nonce,
                binding.request_id,
                binding.tenant_id,
                binding.repository_id,
                binding.node_id,
                binding.public_key_fingerprint,
            ),
            &signature,
        )
        .is_ok()
}

// Its own domain, distinct from activation and envelope signing (ADR-0098
// decision 1): a signature over one of those must never verify as a
// connection challenge response, or a replayed activation/envelope signature
// could open a live stream it was never meant to authenticate.
const CONNECTION_DOMAIN: &[u8] = b"mindleak.ackplane.v1.node_sync.connection\0";

/// The immutable values a `Synchronize` connection's challenge binds together.
pub struct ConnectionChallengeBinding<'a> {
    pub nonce: &'a [u8],
    pub tenant_id: &'a str,
    pub repository_id: &'a str,
    pub producer_id: &'a str,
    pub signing_key_id: &'a str,
}

/// Encode the exact domain-separated bytes a node signs to prove it holds the
/// key it named in `Hello`, over this connection's nonce. Same
/// length-delimited construction as [`activation_challenge_bytes`], so no
/// field can be reinterpreted as part of an adjacent one.
pub fn connection_challenge_bytes(binding: &ConnectionChallengeBinding<'_>) -> Vec<u8> {
    let fields = [
        binding.nonce,
        binding.tenant_id.as_bytes(),
        binding.repository_id.as_bytes(),
        binding.producer_id.as_bytes(),
        binding.signing_key_id.as_bytes(),
    ];
    let mut bytes = Vec::with_capacity(
        CONNECTION_DOMAIN.len() + fields.iter().map(|field| 4 + field.len()).sum::<usize>(),
    );
    bytes.extend_from_slice(CONNECTION_DOMAIN);
    for field in fields {
        bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
        bytes.extend_from_slice(field);
    }
    bytes
}

/// Verify a node's proof of possession of the enrolled key it named, over the
/// nonce this connection issued.
pub fn verify_connection_challenge(
    public_key: &[u8],
    signature: &[u8],
    binding: ConnectionChallengeBinding<'_>,
) -> bool {
    let Ok(public_key) = <&[u8; 32]>::try_from(public_key) else {
        return false;
    };
    let Ok(verifying_key) = VerifyingKey::from_bytes(public_key) else {
        return false;
    };
    let Ok(signature) = Signature::from_slice(signature) else {
        return false;
    };
    verifying_key
        .verify(&connection_challenge_bytes(&binding), &signature)
        .is_ok()
}

const KEY_ROTATION_DOMAIN: &[u8] = b"mindleak.ackplane.v1.enrollment.key_rotation\0";

/// The immutable values a rotation statement binds together (ADR-0085
/// decision 7), bundled so encoding them takes one argument instead of eight.
pub struct KeyRotationStatement<'a> {
    pub tenant_id: &'a str,
    pub repository_id: &'a str,
    pub node_id: &'a str,
    pub current_key_id: &'a str,
    pub successor_key_id: &'a str,
    pub successor_public_key_fingerprint: &'a str,
    pub successor_public_key: &'a [u8],
    pub requested_overlap_seconds: u64,
}

/// Encode the exact domain-separated bytes both the current and successor key
/// must sign to authorise a rotation (ADR-0085 decision 7). Both signatures
/// cover the identical bytes: verifying each against its own key is what
/// proves continuity between them, rather than merely proving each key exists.
pub fn key_rotation_bytes(statement: &KeyRotationStatement<'_>) -> Vec<u8> {
    let fields: [&[u8]; 7] = [
        statement.tenant_id.as_bytes(),
        statement.repository_id.as_bytes(),
        statement.node_id.as_bytes(),
        statement.current_key_id.as_bytes(),
        statement.successor_key_id.as_bytes(),
        statement.successor_public_key_fingerprint.as_bytes(),
        statement.successor_public_key,
    ];
    let overlap_bytes = statement.requested_overlap_seconds.to_be_bytes();
    let mut bytes = Vec::with_capacity(
        KEY_ROTATION_DOMAIN.len()
            + fields.iter().map(|field| 4 + field.len()).sum::<usize>()
            + overlap_bytes.len(),
    );
    bytes.extend_from_slice(KEY_ROTATION_DOMAIN);
    for field in fields {
        bytes.extend_from_slice(&(field.len() as u32).to_be_bytes());
        bytes.extend_from_slice(field);
    }
    bytes.extend_from_slice(&overlap_bytes);
    bytes
}

/// Verify one key's signature over rotation bytes. The caller invokes this
/// once with the current key and once with the successor key, both against
/// the identical bytes from [`key_rotation_bytes`]; continuity holds only when
/// both pass.
pub fn verify_key_rotation_signature(public_key: &[u8], signature: &[u8], bytes: &[u8]) -> bool {
    let Ok(public_key) = <&[u8; 32]>::try_from(public_key) else {
        return false;
    };
    let Ok(verifying_key) = VerifyingKey::from_bytes(public_key) else {
        return false;
    };
    let Ok(signature) = Signature::from_slice(signature) else {
        return false;
    };
    verifying_key.verify(bytes, &signature).is_ok()
}

/// The authority-owned state of a node enrollment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnrollmentState {
    Pending,
    Approved,
    Activating,
    Active,
    Expired,
    Rejected,
    Revoked,
}

/// The stable identity and approved public key for an enrollment request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enrollment {
    pub request_id: String,
    pub tenant_id: String,
    pub repository_id: String,
    pub node_id: String,
    pub public_key_fingerprint: String,
    pub public_key: Vec<u8>,
    pub state: EnrollmentState,
}

/// A single-use proof-of-possession challenge issued by Ackplane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationChallenge {
    pub request_id: String,
    pub nonce: Vec<u8>,
    pub expires_at: SystemTime,
    pub consumed: bool,
}

/// Why a presented activation proof cannot activate an enrollment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationFailure {
    NotApproved,
    RequestMismatch,
    ChallengeExpired,
    ChallengeReplayed,
    InvalidProof,
}

impl Enrollment {
    /// Begin activation only after the authority verifies a fresh, unused
    /// challenge against the public key it already approved and stored.
    pub fn activate(
        &mut self,
        challenge: &mut ActivationChallenge,
        now: SystemTime,
        proof_is_valid: bool,
    ) -> Result<(), ActivationFailure> {
        if self.state != EnrollmentState::Approved {
            return Err(ActivationFailure::NotApproved);
        }
        if challenge.request_id != self.request_id {
            return Err(ActivationFailure::RequestMismatch);
        }
        if challenge.consumed {
            return Err(ActivationFailure::ChallengeReplayed);
        }
        if now > challenge.expires_at {
            return Err(ActivationFailure::ChallengeExpired);
        }
        if !proof_is_valid {
            return Err(ActivationFailure::InvalidProof);
        }

        challenge.consumed = true;
        self.state = EnrollmentState::Activating;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use ed25519_dalek::{Signer, SigningKey};

    use super::{
        activation_challenge_bytes, connection_challenge_bytes, key_rotation_bytes,
        public_key_fingerprint, verify_activation_proof, verify_connection_challenge,
        verify_key_rotation_signature, ActivationChallenge, ActivationFailure,
        ActivationProofBinding, ConnectionChallengeBinding, Enrollment, EnrollmentState,
        KeyRotationStatement,
    };

    fn approved_enrollment() -> Enrollment {
        Enrollment {
            request_id: "request-1".to_owned(),
            tenant_id: "tenant-1".to_owned(),
            repository_id: "repository-1".to_owned(),
            node_id: "node-1".to_owned(),
            public_key_fingerprint: "fingerprint-1".to_owned(),
            public_key: vec![1, 2, 3],
            state: EnrollmentState::Approved,
        }
    }

    fn unconsumed_challenge() -> ActivationChallenge {
        ActivationChallenge {
            request_id: "request-1".to_owned(),
            nonce: vec![4, 5, 6],
            expires_at: SystemTime::UNIX_EPOCH + Duration::from_secs(60),
            consumed: false,
        }
    }

    #[test]
    fn valid_proof_begins_activation_and_consumes_challenge() {
        let mut enrollment = approved_enrollment();
        let mut challenge = unconsumed_challenge();

        let result = enrollment.activate(&mut challenge, SystemTime::UNIX_EPOCH, true);

        assert_eq!(result, Ok(()));
        assert_eq!(enrollment.state, EnrollmentState::Activating);
        assert!(challenge.consumed);
    }

    #[test]
    fn invalid_proof_leaves_enrollment_approved_and_challenge_unconsumed() {
        let mut enrollment = approved_enrollment();
        let mut challenge = unconsumed_challenge();

        let result = enrollment.activate(&mut challenge, SystemTime::UNIX_EPOCH, false);

        assert_eq!(result, Err(ActivationFailure::InvalidProof));
        assert_eq!(enrollment.state, EnrollmentState::Approved);
        assert!(!challenge.consumed);
    }

    #[test]
    fn mismatched_challenge_request_leaves_enrollment_approved_and_challenge_unconsumed() {
        let mut enrollment = approved_enrollment();
        let mut challenge = unconsumed_challenge();
        challenge.request_id = "another-request".to_owned();

        let result = enrollment.activate(&mut challenge, SystemTime::UNIX_EPOCH, true);

        assert_eq!(result, Err(ActivationFailure::RequestMismatch));
        assert_eq!(enrollment.state, EnrollmentState::Approved);
        assert!(!challenge.consumed);
    }

    #[test]
    fn expired_challenge_leaves_enrollment_approved_and_challenge_unconsumed() {
        let mut enrollment = approved_enrollment();
        let mut challenge = unconsumed_challenge();

        let result = enrollment.activate(
            &mut challenge,
            SystemTime::UNIX_EPOCH + Duration::from_secs(61),
            true,
        );

        assert_eq!(result, Err(ActivationFailure::ChallengeExpired));
        assert_eq!(enrollment.state, EnrollmentState::Approved);
        assert!(!challenge.consumed);
    }

    #[test]
    fn replayed_challenge_leaves_enrollment_approved() {
        let mut enrollment = approved_enrollment();
        let mut challenge = unconsumed_challenge();
        challenge.consumed = true;

        let result = enrollment.activate(&mut challenge, SystemTime::UNIX_EPOCH, true);

        assert_eq!(result, Err(ActivationFailure::ChallengeReplayed));
        assert_eq!(enrollment.state, EnrollmentState::Approved);
        assert!(challenge.consumed);
    }

    #[test]
    fn unapproved_enrollment_rejects_proof_without_consuming_challenge() {
        let mut enrollment = approved_enrollment();
        let mut challenge = unconsumed_challenge();
        enrollment.state = EnrollmentState::Pending;

        let result = enrollment.activate(&mut challenge, SystemTime::UNIX_EPOCH, true);

        assert_eq!(result, Err(ActivationFailure::NotApproved));
        assert_eq!(enrollment.state, EnrollmentState::Pending);
        assert!(!challenge.consumed);
    }

    #[test]
    fn activation_challenge_encoding_binds_each_field_unambiguously() {
        let encoded = activation_challenge_bytes(
            &[1, 2],
            "request",
            "tenant",
            "repository",
            "node",
            "fingerprint",
        );

        assert_eq!(
            encoded,
            [
                b"mindleak.ackplane.v1.enrollment.activation\0".as_slice(),
                &[0, 0, 0, 2, 1, 2],
                &[0, 0, 0, 7],
                b"request",
                &[0, 0, 0, 6],
                b"tenant",
                &[0, 0, 0, 10],
                b"repository",
                &[0, 0, 0, 4],
                b"node",
                &[0, 0, 0, 11],
                b"fingerprint",
            ]
            .concat()
        );
    }

    #[test]
    fn proof_verification_rejects_a_signature_reused_for_another_node() {
        let signing_key = SigningKey::from_bytes(&[9; 32]);
        let nonce = [1, 2, 3];
        let signature = signing_key.sign(&activation_challenge_bytes(
            &nonce,
            "request",
            "tenant",
            "repository",
            "node-1",
            "fingerprint",
        ));

        let valid = verify_activation_proof(
            &signing_key.verifying_key().to_bytes(),
            &signature.to_bytes(),
            ActivationProofBinding {
                nonce: &nonce,
                request_id: "request",
                tenant_id: "tenant",
                repository_id: "repository",
                node_id: "node-1",
                public_key_fingerprint: "fingerprint",
            },
        );
        let reused_for_another_node = verify_activation_proof(
            &signing_key.verifying_key().to_bytes(),
            &signature.to_bytes(),
            ActivationProofBinding {
                nonce: &nonce,
                request_id: "request",
                tenant_id: "tenant",
                repository_id: "repository",
                node_id: "node-2",
                public_key_fingerprint: "fingerprint",
            },
        );

        assert_eq!((valid, reused_for_another_node), (true, false));
    }

    #[test]
    fn public_key_fingerprint_is_prefixed_sha256_hex() {
        assert_eq!(
            public_key_fingerprint(&[0; 32]),
            "ed25519:66687aadf862bd776c8fc18b8e9f8e20089714856ee233b3902a591d0d5f2925"
        );
    }

    #[test]
    fn key_rotation_encoding_binds_each_field_unambiguously() {
        let encoded = key_rotation_bytes(&KeyRotationStatement {
            tenant_id: "tenant",
            repository_id: "repository",
            node_id: "node",
            current_key_id: "key-current",
            successor_key_id: "key-successor",
            successor_public_key_fingerprint: "fingerprint",
            successor_public_key: &[9, 9],
            requested_overlap_seconds: 3_600,
        });

        assert_eq!(
            encoded,
            [
                b"mindleak.ackplane.v1.enrollment.key_rotation\0".as_slice(),
                &[0, 0, 0, 6],
                b"tenant",
                &[0, 0, 0, 10],
                b"repository",
                &[0, 0, 0, 4],
                b"node",
                &[0, 0, 0, 11],
                b"key-current",
                &[0, 0, 0, 13],
                b"key-successor",
                &[0, 0, 0, 11],
                b"fingerprint",
                &[0, 0, 0, 2, 9, 9],
                &3_600_u64.to_be_bytes(),
            ]
            .concat()
        );
    }

    /// The load-bearing property of decision 7: a rotation is authorised only
    /// when BOTH the current key and the successor key sign the identical
    /// statement. A signature valid for one node's rotation must not verify
    /// for a different node's, or the continuity proof would prove nothing
    /// about which node is rotating.
    #[test]
    fn continuity_proof_ties_both_signatures_to_the_same_rotation_statement() {
        let current = SigningKey::from_bytes(&[1; 32]);
        let successor = SigningKey::from_bytes(&[2; 32]);
        let bytes = key_rotation_bytes(&KeyRotationStatement {
            tenant_id: "tenant",
            repository_id: "repository",
            node_id: "node-1",
            current_key_id: "key-current",
            successor_key_id: "key-successor",
            successor_public_key_fingerprint: &public_key_fingerprint(
                &successor.verifying_key().to_bytes(),
            ),
            successor_public_key: &successor.verifying_key().to_bytes(),
            requested_overlap_seconds: 3_600,
        });
        let current_signature = current.sign(&bytes);
        let successor_signature = successor.sign(&bytes);

        assert!(verify_key_rotation_signature(
            &current.verifying_key().to_bytes(),
            &current_signature.to_bytes(),
            &bytes,
        ));
        assert!(verify_key_rotation_signature(
            &successor.verifying_key().to_bytes(),
            &successor_signature.to_bytes(),
            &bytes,
        ));

        let bytes_for_another_node = key_rotation_bytes(&KeyRotationStatement {
            tenant_id: "tenant",
            repository_id: "repository",
            node_id: "node-2",
            current_key_id: "key-current",
            successor_key_id: "key-successor",
            successor_public_key_fingerprint: &public_key_fingerprint(
                &successor.verifying_key().to_bytes(),
            ),
            successor_public_key: &successor.verifying_key().to_bytes(),
            requested_overlap_seconds: 3_600,
        });
        assert!(!verify_key_rotation_signature(
            &current.verifying_key().to_bytes(),
            &current_signature.to_bytes(),
            &bytes_for_another_node,
        ));
    }

    #[test]
    fn a_successor_signature_does_not_verify_against_the_current_key() {
        let current = SigningKey::from_bytes(&[1; 32]);
        let successor = SigningKey::from_bytes(&[2; 32]);
        let bytes = key_rotation_bytes(&KeyRotationStatement {
            tenant_id: "tenant",
            repository_id: "repository",
            node_id: "node-1",
            current_key_id: "key-current",
            successor_key_id: "key-successor",
            successor_public_key_fingerprint: &public_key_fingerprint(
                &successor.verifying_key().to_bytes(),
            ),
            successor_public_key: &successor.verifying_key().to_bytes(),
            requested_overlap_seconds: 3_600,
        });
        let successor_signature = successor.sign(&bytes);

        assert!(!verify_key_rotation_signature(
            &current.verifying_key().to_bytes(),
            &successor_signature.to_bytes(),
            &bytes,
        ));
    }

    fn connection_binding<'a>(
        nonce: &'a [u8],
        signing_key_id: &'a str,
    ) -> ConnectionChallengeBinding<'a> {
        ConnectionChallengeBinding {
            nonce,
            tenant_id: "tenant",
            repository_id: "repository",
            producer_id: "node-1",
            signing_key_id,
        }
    }

    #[test]
    fn connection_challenge_encoding_binds_each_field_unambiguously() {
        let encoded = connection_challenge_bytes(&connection_binding(&[7, 8], "key-1"));

        assert_eq!(
            encoded,
            [
                b"mindleak.ackplane.v1.node_sync.connection\0".as_slice(),
                &[0, 0, 0, 2, 7, 8],
                &[0, 0, 0, 6],
                b"tenant",
                &[0, 0, 0, 10],
                b"repository",
                &[0, 0, 0, 6],
                b"node-1",
                &[0, 0, 0, 5],
                b"key-1",
            ]
            .concat()
        );
    }

    #[test]
    fn a_valid_connection_response_verifies_against_its_own_key() {
        let key = SigningKey::from_bytes(&[9; 32]);
        let nonce = [1, 2, 3];
        let signature = key.sign(&connection_challenge_bytes(&connection_binding(
            &nonce, "key-1",
        )));

        assert!(verify_connection_challenge(
            &key.verifying_key().to_bytes(),
            &signature.to_bytes(),
            connection_binding(&nonce, "key-1"),
        ));
    }

    /// Decision 1's whole point: a connection challenge is its own domain, so
    /// a signature produced for enrolment activation (same key, same nonce
    /// bytes reused as a raw nonce) must not verify as a connection response,
    /// and vice versa. Without domain separation, a captured activation proof
    /// could be replayed to open a live stream it was never meant to
    /// authenticate.
    #[test]
    fn a_connection_response_does_not_verify_as_an_activation_proof_or_the_reverse() {
        let key = SigningKey::from_bytes(&[9; 32]);
        let nonce = [1, 2, 3];

        let connection_signature = key.sign(&connection_challenge_bytes(&connection_binding(
            &nonce, "key-1",
        )));
        let activation_signature = key.sign(&activation_challenge_bytes(
            &nonce,
            "request",
            "tenant",
            "repository",
            "node-1",
            "key-1",
        ));

        assert!(!verify_activation_proof(
            &key.verifying_key().to_bytes(),
            &connection_signature.to_bytes(),
            ActivationProofBinding {
                nonce: &nonce,
                request_id: "request",
                tenant_id: "tenant",
                repository_id: "repository",
                node_id: "node-1",
                public_key_fingerprint: "key-1",
            },
        ));
        assert!(!verify_connection_challenge(
            &key.verifying_key().to_bytes(),
            &activation_signature.to_bytes(),
            connection_binding(&nonce, "key-1"),
        ));
    }

    #[test]
    fn connection_response_signed_for_a_different_node_does_not_verify() {
        let key = SigningKey::from_bytes(&[9; 32]);
        let nonce = [1, 2, 3];
        let signature = key.sign(&connection_challenge_bytes(&ConnectionChallengeBinding {
            nonce: &nonce,
            tenant_id: "tenant",
            repository_id: "repository",
            producer_id: "node-1",
            signing_key_id: "key-1",
        }));

        assert!(!verify_connection_challenge(
            &key.verifying_key().to_bytes(),
            &signature.to_bytes(),
            ConnectionChallengeBinding {
                nonce: &nonce,
                tenant_id: "tenant",
                repository_id: "repository",
                producer_id: "node-2",
                signing_key_id: "key-1",
            },
        ));
    }
}

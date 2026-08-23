//! The exact bytes a candidate node signs to prove possession of a binding
//! before `CheckEnrollmentStatus` will reveal its lifecycle state (ADR-0122).
//!
//! Keyed on the candidate's self-computed public-key fingerprint rather than
//! a server-assigned `signing_key_id` -- a node that has never submitted an
//! enrollment request has no `signing_key_id` yet, but it always already
//! holds the Ed25519 key it generated locally (ADR-0085 decision 2) and can
//! compute that key's fingerprint itself.

use crate::signing_bytes::push_field;
use crate::v1;

/// Domain separation for enrollment-status-check signatures. Distinct from
/// `claim_auth::CLAIM_DOMAIN` and `knowledge_auth::KNOWLEDGE_DOMAIN` so a
/// signature produced for one domain can never verify as another, even if
/// every other field happened to coincide.
pub const ENROLLMENT_STATUS_DOMAIN: &[u8] = b"mindleak.ackplane.v1.enrollment_status\0";

/// `CheckEnrollmentStatus` is this domain's only operation today. A distinct
/// type (rather than a bare tag constant) mirrors `ClaimOperation`/
/// `KnowledgeOperation`'s shape so a second operation, if one is ever added,
/// extends this enum instead of changing the signing-bytes function's
/// signature.
#[derive(Debug, Clone, Copy)]
pub enum EnrollmentStatusOperation {
    Check,
}

impl EnrollmentStatusOperation {
    /// The distinct tag this variant signs, so a signature for one operation
    /// can never verify for another even when every other field coincides.
    fn tag(self) -> &'static str {
        match self {
            Self::Check => "check",
        }
    }
}

/// Binds the authentication to this specific request's identity -- tenant,
/// repository, the exact candidate node and key fingerprint being asked
/// about -- and to the operation being authorized. Every field is
/// length-delimited, following `claim_auth::claim_signing_bytes`.
pub fn enrollment_status_signing_bytes(
    tenant_id: &str,
    repository_id: &str,
    operation: EnrollmentStatusOperation,
    authentication: &v1::EnrollmentStatusAuthentication,
) -> Vec<u8> {
    let fields: [&[u8]; 6] = [
        authentication.node_id.as_bytes(),
        authentication.key_fingerprint.as_bytes(),
        authentication.signed_at.as_bytes(),
        &authentication.nonce,
        tenant_id.as_bytes(),
        repository_id.as_bytes(),
    ];

    let mut bytes = Vec::with_capacity(ENROLLMENT_STATUS_DOMAIN.len() + 64);
    bytes.extend_from_slice(ENROLLMENT_STATUS_DOMAIN);
    for field in fields {
        push_field(&mut bytes, field);
    }
    push_field(&mut bytes, operation.tag().as_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authentication(nonce: u8) -> v1::EnrollmentStatusAuthentication {
        v1::EnrollmentStatusAuthentication {
            node_id: "node-1".to_string(),
            key_fingerprint: "fingerprint-1".to_string(),
            signed_at: "2026-01-01T00:00:00Z".to_string(),
            nonce: vec![nonce; 16],
            signature: Vec::new(),
        }
    }

    #[test]
    fn identical_inputs_produce_identical_bytes() {
        let a = enrollment_status_signing_bytes(
            "tenant-a",
            "repo-a",
            EnrollmentStatusOperation::Check,
            &authentication(1),
        );
        let b = enrollment_status_signing_bytes(
            "tenant-a",
            "repo-a",
            EnrollmentStatusOperation::Check,
            &authentication(1),
        );
        assert_eq!(a, b);
    }

    #[test]
    fn a_different_node_id_changes_the_bytes() {
        let a = enrollment_status_signing_bytes(
            "tenant-a",
            "repo-a",
            EnrollmentStatusOperation::Check,
            &authentication(1),
        );
        let mut changed = authentication(1);
        changed.node_id = "node-2".to_string();
        let b = enrollment_status_signing_bytes(
            "tenant-a",
            "repo-a",
            EnrollmentStatusOperation::Check,
            &changed,
        );
        assert_ne!(a, b);
    }

    #[test]
    fn a_different_key_fingerprint_changes_the_bytes() {
        let a = enrollment_status_signing_bytes(
            "tenant-a",
            "repo-a",
            EnrollmentStatusOperation::Check,
            &authentication(1),
        );
        let mut changed = authentication(1);
        changed.key_fingerprint = "fingerprint-2".to_string();
        let b = enrollment_status_signing_bytes(
            "tenant-a",
            "repo-a",
            EnrollmentStatusOperation::Check,
            &changed,
        );
        assert_ne!(a, b);
    }

    #[test]
    fn a_different_nonce_changes_the_bytes() {
        let a = enrollment_status_signing_bytes(
            "tenant-a",
            "repo-a",
            EnrollmentStatusOperation::Check,
            &authentication(1),
        );
        let b = enrollment_status_signing_bytes(
            "tenant-a",
            "repo-a",
            EnrollmentStatusOperation::Check,
            &authentication(2),
        );
        assert_ne!(a, b);
    }

    #[test]
    fn a_different_tenant_changes_the_bytes() {
        let a = enrollment_status_signing_bytes(
            "tenant-a",
            "repo-a",
            EnrollmentStatusOperation::Check,
            &authentication(1),
        );
        let b = enrollment_status_signing_bytes(
            "tenant-b",
            "repo-a",
            EnrollmentStatusOperation::Check,
            &authentication(1),
        );
        assert_ne!(a, b);
    }

    #[test]
    fn a_different_repository_changes_the_bytes() {
        let a = enrollment_status_signing_bytes(
            "tenant-a",
            "repo-a",
            EnrollmentStatusOperation::Check,
            &authentication(1),
        );
        let b = enrollment_status_signing_bytes(
            "tenant-a",
            "repo-b",
            EnrollmentStatusOperation::Check,
            &authentication(1),
        );
        assert_ne!(a, b);
    }

    #[test]
    fn a_different_signed_at_changes_the_bytes() {
        let a = enrollment_status_signing_bytes(
            "tenant-a",
            "repo-a",
            EnrollmentStatusOperation::Check,
            &authentication(1),
        );
        let mut changed = authentication(1);
        changed.signed_at = "2026-01-02T00:00:00Z".to_string();
        let b = enrollment_status_signing_bytes(
            "tenant-a",
            "repo-a",
            EnrollmentStatusOperation::Check,
            &changed,
        );
        assert_ne!(a, b);
    }
}

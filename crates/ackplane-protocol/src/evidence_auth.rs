//! Exact signed bytes for EvidenceService requests.
//!
//! Evidence is not raw terminal output or a source blob. Its signer binds the
//! task, closed evidence kind, bounded reference, SHA-256 digest, observation
//! time, and agent session so an in-flight request cannot be retargeted.

use crate::signing_bytes::push_field;
use crate::v1;

/// Domain separation for EvidenceService request signatures.
pub const EVIDENCE_DOMAIN: &[u8] = b"mindleak.ackplane.v1.evidence\0";

/// Which EvidenceService operation an authentication authorizes.
#[derive(Debug, Clone, Copy)]
pub enum EvidenceOperation<'a> {
    Record {
        task_id: &'a str,
        evidence_kind: i32,
        source_ref: &'a str,
        content_digest: &'a [u8],
        observed_at: &'a str,
        agent_session_id: &'a str,
    },
    List {
        task_id: &'a str,
        limit: u32,
    },
}

impl EvidenceOperation<'_> {
    fn tag(&self) -> &'static str {
        match self {
            Self::Record { .. } => "record",
            Self::List { .. } => "list",
        }
    }

    fn push_fields(&self, bytes: &mut Vec<u8>) {
        push_field(bytes, self.tag().as_bytes());
        match self {
            Self::Record {
                task_id,
                evidence_kind,
                source_ref,
                content_digest,
                observed_at,
                agent_session_id,
            } => {
                push_field(bytes, task_id.as_bytes());
                push_field(bytes, &evidence_kind.to_be_bytes());
                push_field(bytes, source_ref.as_bytes());
                push_field(bytes, content_digest);
                push_field(bytes, observed_at.as_bytes());
                push_field(bytes, agent_session_id.as_bytes());
            }
            Self::List { task_id, limit } => {
                push_field(bytes, task_id.as_bytes());
                push_field(bytes, &limit.to_be_bytes());
            }
        }
    }
}

/// Binds an EvidenceAuthentication to one tenant, repository, and exact
/// operation. Every field is length-delimited so changing one field cannot
/// alter how another is parsed.
pub fn evidence_signing_bytes(
    tenant_id: &str,
    repository_id: &str,
    operation: &EvidenceOperation,
    authentication: &v1::EvidenceAuthentication,
) -> Vec<u8> {
    let identity_fields: [&[u8]; 6] = [
        authentication.signing_key_id.as_bytes(),
        authentication.node_id.as_bytes(),
        authentication.signed_at.as_bytes(),
        &authentication.nonce,
        tenant_id.as_bytes(),
        repository_id.as_bytes(),
    ];

    let mut bytes = Vec::with_capacity(EVIDENCE_DOMAIN.len() + 128);
    bytes.extend_from_slice(EVIDENCE_DOMAIN);
    for field in identity_fields {
        push_field(&mut bytes, field);
    }
    operation.push_fields(&mut bytes);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authentication(nonce: u8) -> v1::EvidenceAuthentication {
        v1::EvidenceAuthentication {
            signing_key_id: "key-1".to_string(),
            node_id: "node-1".to_string(),
            signed_at: "2026-01-01T00:00:00Z".to_string(),
            nonce: vec![nonce; 16],
            signature: Vec::new(),
        }
    }

    const RECORD: EvidenceOperation<'static> = EvidenceOperation::Record {
        task_id: "task:123",
        evidence_kind: 1,
        source_ref: "commit:0123456789abcdef",
        content_digest: b"01234567890123456789012345678901",
        observed_at: "2026-01-01T00:00:00Z",
        agent_session_id: "session:v1:agent",
    };

    #[test]
    fn identical_inputs_produce_identical_bytes() {
        let first = evidence_signing_bytes("tenant-a", "repo-a", &RECORD, &authentication(1));
        let second = evidence_signing_bytes("tenant-a", "repo-a", &RECORD, &authentication(1));

        assert_eq!(first, second);
    }

    #[test]
    fn record_and_list_never_share_a_signature_shape() {
        let record = evidence_signing_bytes("tenant-a", "repo-a", &RECORD, &authentication(1));
        let list = evidence_signing_bytes(
            "tenant-a",
            "repo-a",
            &EvidenceOperation::List {
                task_id: "task:123",
                limit: 20,
            },
            &authentication(1),
        );

        assert_ne!(record, list);
    }

    #[test]
    fn changing_a_digest_or_task_changes_record_bytes() {
        let original = evidence_signing_bytes("tenant-a", "repo-a", &RECORD, &authentication(1));
        let changed_digest = EvidenceOperation::Record {
            task_id: "task:123",
            evidence_kind: 1,
            source_ref: "commit:0123456789abcdef",
            content_digest: b"11234567890123456789012345678901",
            observed_at: "2026-01-01T00:00:00Z",
            agent_session_id: "session:v1:agent",
        };
        let changed_task = EvidenceOperation::Record {
            task_id: "task:456",
            evidence_kind: 1,
            source_ref: "commit:0123456789abcdef",
            content_digest: b"01234567890123456789012345678901",
            observed_at: "2026-01-01T00:00:00Z",
            agent_session_id: "session:v1:agent",
        };

        assert_ne!(
            original,
            evidence_signing_bytes("tenant-a", "repo-a", &changed_digest, &authentication(1))
        );
        assert_ne!(
            original,
            evidence_signing_bytes("tenant-a", "repo-a", &changed_task, &authentication(1))
        );
    }

    #[test]
    fn evidence_domain_never_shares_claim_domain_bytes() {
        let bytes = evidence_signing_bytes("tenant-a", "repo-a", &RECORD, &authentication(1));

        assert!(bytes.starts_with(EVIDENCE_DOMAIN));
        assert_ne!(EVIDENCE_DOMAIN, crate::claim_auth::CLAIM_DOMAIN);
    }
}

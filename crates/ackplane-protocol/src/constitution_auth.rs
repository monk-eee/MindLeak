//! The exact bytes a repository node signs to authenticate a
//! ConstitutionService request, mirroring `knowledge_auth`'s shape with its
//! own domain and operation fields -- a constitution snapshot has no branch,
//! lease, or knowledge-content field to bind, so reusing another domain's
//! operation type would sign nothing meaningful for this one.

use crate::signing_bytes::push_field;
use crate::v1;

/// Domain separation for constitution-request signatures. Distinct from
/// `claim_auth::CLAIM_DOMAIN` and `knowledge_auth::KNOWLEDGE_DOMAIN` so a
/// signature produced for one domain can never verify as another, even if
/// every other field happened to coincide.
pub const CONSTITUTION_DOMAIN: &[u8] = b"mindleak.ackplane.v1.constitution\0";

/// Which `ConstitutionService` RPC an authentication authorizes, and that
/// operation's own fields.
///
/// Identity alone (tenant/repository/node/key) proves *who* is asking, not
/// *what* they asked for: binding the operation tag and every
/// operation-specific field closes that gap, the same reasoning ADR-0100
/// decision 10 already applied to claims and ADR-0108 applied to knowledge.
#[derive(Debug, Clone, Copy)]
pub enum ConstitutionOperation<'a> {
    Publish {
        version_id: &'a str,
        version: u32,
        status: &'a str,
        /// The clause count, not each clause's full text -- a published
        /// snapshot can carry many clauses, and the point of binding this is
        /// to catch a version/status mismatch, not to sign the entire
        /// document (the same "bind what identifies the request, not the
        /// full payload" reasoning `Recall`'s `query_embedding_present`
        /// already uses for a large ranking input).
        clause_count: u32,
    },
    GetActive,
}

impl ConstitutionOperation<'_> {
    /// The distinct tag every variant signs, so a signature for one RPC can
    /// never verify for another even when every other field happens to
    /// coincide.
    fn tag(&self) -> &'static str {
        match self {
            Self::Publish { .. } => "publish",
            Self::GetActive => "get_active",
        }
    }

    fn push_fields(&self, bytes: &mut Vec<u8>) {
        push_field(bytes, self.tag().as_bytes());
        if let Self::Publish {
            version_id,
            version,
            status,
            clause_count,
        } = self
        {
            push_field(bytes, version_id.as_bytes());
            push_field(bytes, &version.to_be_bytes());
            push_field(bytes, status.as_bytes());
            push_field(bytes, &clause_count.to_be_bytes());
        }
    }
}

/// Binds the authentication to this specific request's identity -- tenant and
/// repository -- and to the exact operation and fields being authorized.
/// Every field is length-delimited, following `knowledge_auth::knowledge_signing_bytes`.
pub fn constitution_signing_bytes(
    tenant_id: &str,
    repository_id: &str,
    operation: &ConstitutionOperation,
    authentication: &v1::ConstitutionAuthentication,
) -> Vec<u8> {
    let identity_fields: [&[u8]; 6] = [
        authentication.signing_key_id.as_bytes(),
        authentication.node_id.as_bytes(),
        authentication.signed_at.as_bytes(),
        &authentication.nonce,
        tenant_id.as_bytes(),
        repository_id.as_bytes(),
    ];

    let mut bytes = Vec::with_capacity(CONSTITUTION_DOMAIN.len() + 64);
    bytes.extend_from_slice(CONSTITUTION_DOMAIN);
    for field in identity_fields {
        push_field(&mut bytes, field);
    }
    operation.push_fields(&mut bytes);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authentication(nonce: u8) -> v1::ConstitutionAuthentication {
        v1::ConstitutionAuthentication {
            signing_key_id: "key-1".to_string(),
            node_id: "node-1".to_string(),
            signed_at: "2026-01-01T00:00:00Z".to_string(),
            nonce: vec![nonce; 16],
            signature: Vec::new(),
        }
    }

    const PUBLISH: ConstitutionOperation<'static> = ConstitutionOperation::Publish {
        version_id: "version-1",
        version: 4,
        status: "active",
        clause_count: 12,
    };

    #[test]
    fn identical_inputs_produce_identical_bytes() {
        let a = constitution_signing_bytes("tenant-a", "repo-a", &PUBLISH, &authentication(1));
        let b = constitution_signing_bytes("tenant-a", "repo-a", &PUBLISH, &authentication(1));
        assert_eq!(a, b);
    }

    #[test]
    fn a_different_operation_tag_changes_the_bytes() {
        let publish =
            constitution_signing_bytes("tenant-a", "repo-a", &PUBLISH, &authentication(1));
        let get_active = constitution_signing_bytes(
            "tenant-a",
            "repo-a",
            &ConstitutionOperation::GetActive,
            &authentication(1),
        );
        assert_ne!(publish, get_active);
    }

    #[test]
    fn a_different_nonce_changes_the_bytes() {
        let a = constitution_signing_bytes("tenant-a", "repo-a", &PUBLISH, &authentication(1));
        let b = constitution_signing_bytes("tenant-a", "repo-a", &PUBLISH, &authentication(2));
        assert_ne!(a, b);
    }

    #[test]
    fn a_changed_operation_field_changes_the_bytes() {
        let a = constitution_signing_bytes("tenant-a", "repo-a", &PUBLISH, &authentication(1));
        let changed = ConstitutionOperation::Publish {
            version_id: "version-1",
            version: 5,
            status: "active",
            clause_count: 12,
        };
        let b = constitution_signing_bytes("tenant-a", "repo-a", &changed, &authentication(1));
        assert_ne!(a, b);
    }

    #[test]
    fn publish_binds_the_clause_count_not_just_the_version() {
        let fewer_clauses = constitution_signing_bytes(
            "tenant-a",
            "repo-a",
            &ConstitutionOperation::Publish {
                version_id: "version-1",
                version: 4,
                status: "active",
                clause_count: 12,
            },
            &authentication(1),
        );
        let more_clauses = constitution_signing_bytes(
            "tenant-a",
            "repo-a",
            &ConstitutionOperation::Publish {
                version_id: "version-1",
                version: 4,
                status: "active",
                clause_count: 13,
            },
            &authentication(1),
        );
        assert_ne!(fewer_clauses, more_clauses);
    }

    #[test]
    fn a_constitution_signature_never_shares_bytes_with_another_domains_separator() {
        let bytes = constitution_signing_bytes("tenant-a", "repo-a", &PUBLISH, &authentication(1));
        assert!(bytes.starts_with(CONSTITUTION_DOMAIN));
        assert_ne!(CONSTITUTION_DOMAIN, crate::claim_auth::CLAIM_DOMAIN);
        assert_ne!(CONSTITUTION_DOMAIN, crate::knowledge_auth::KNOWLEDGE_DOMAIN);
    }
}

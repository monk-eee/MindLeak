//! Domain-separated signatures for authenticated Lifecycle-purge transitions.
//!
//! An enrolled signing key proves control of a registered node credential, not
//! the identity of a particular human. The signed operation therefore binds the
//! exact preview or confirmation request so a credential cannot be replayed for
//! a different destructive action.

use crate::{signing_bytes::push_field, v1};

/// Domain separation for Lifecycle-purge authentication.
pub const LIFECYCLE_PURGE_DOMAIN: &[u8] = b"mindleak.ackplane.v1.lifecycle-purge\0";

/// The exact Lifecycle-purge operation an enrolled credential authorizes.
#[derive(Debug, Clone, Copy)]
pub enum LifecyclePurgeOperation<'a> {
    Preview {
        policy_id: &'a str,
        data_category: &'a str,
        older_than_seconds: u64,
        confirmation_window_seconds: u64,
        idempotency_key: &'a str,
    },
    Confirm {
        request_id: &'a str,
    },
}

impl LifecyclePurgeOperation<'_> {
    fn tag(&self) -> &'static str {
        match self {
            Self::Preview { .. } => "preview",
            Self::Confirm { .. } => "confirm",
        }
    }

    fn push_fields(&self, bytes: &mut Vec<u8>) {
        push_field(bytes, self.tag().as_bytes());
        match self {
            Self::Preview {
                policy_id,
                data_category,
                older_than_seconds,
                confirmation_window_seconds,
                idempotency_key,
            } => {
                push_field(bytes, policy_id.as_bytes());
                push_field(bytes, data_category.as_bytes());
                push_field(bytes, &older_than_seconds.to_be_bytes());
                push_field(bytes, &confirmation_window_seconds.to_be_bytes());
                push_field(bytes, idempotency_key.as_bytes());
            }
            Self::Confirm { request_id } => push_field(bytes, request_id.as_bytes()),
        }
    }
}

/// Binds an enrolled key's authentication to one tenant/repository Lifecycle
/// purge operation and all of that operation's security-relevant fields.
pub fn lifecycle_purge_signing_bytes(
    tenant_id: &str,
    repository_id: &str,
    operation: &LifecyclePurgeOperation<'_>,
    authentication: &v1::ClaimAuthentication,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(LIFECYCLE_PURGE_DOMAIN.len() + 128);
    bytes.extend_from_slice(LIFECYCLE_PURGE_DOMAIN);
    for field in [
        authentication.signing_key_id.as_bytes(),
        authentication.node_id.as_bytes(),
        authentication.signed_at.as_bytes(),
        authentication.nonce.as_slice(),
        tenant_id.as_bytes(),
        repository_id.as_bytes(),
    ] {
        push_field(&mut bytes, field);
    }
    operation.push_fields(&mut bytes);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authentication() -> v1::ClaimAuthentication {
        v1::ClaimAuthentication {
            signing_key_id: "key-a".to_owned(),
            node_id: "node-a".to_owned(),
            signed_at: "2026-08-27T00:00:00Z".to_owned(),
            nonce: vec![7; 16],
            signature: Vec::new(),
        }
    }

    #[test]
    fn preview_signatures_bind_each_mutable_operation_field() {
        let authentication = authentication();
        let preview = LifecyclePurgeOperation::Preview {
            policy_id: "policy-a",
            data_category: "telemetry_events",
            older_than_seconds: 10,
            confirmation_window_seconds: 60,
            idempotency_key: "preview-a",
        };
        let changed = LifecyclePurgeOperation::Preview {
            policy_id: "policy-a",
            data_category: "telemetry_events",
            older_than_seconds: 10,
            confirmation_window_seconds: 61,
            idempotency_key: "preview-a",
        };

        assert_ne!(
            lifecycle_purge_signing_bytes("tenant-a", "repository-a", &preview, &authentication),
            lifecycle_purge_signing_bytes("tenant-a", "repository-a", &changed, &authentication)
        );
    }

    #[test]
    fn preview_and_confirmation_never_share_a_signature_domain() {
        let authentication = authentication();
        let preview = LifecyclePurgeOperation::Preview {
            policy_id: "policy-a",
            data_category: "telemetry_events",
            older_than_seconds: 10,
            confirmation_window_seconds: 60,
            idempotency_key: "request-a",
        };
        let confirm = LifecyclePurgeOperation::Confirm {
            request_id: "request-a",
        };

        assert_ne!(
            lifecycle_purge_signing_bytes("tenant-a", "repository-a", &preview, &authentication),
            lifecycle_purge_signing_bytes("tenant-a", "repository-a", &confirm, &authentication)
        );
    }
}

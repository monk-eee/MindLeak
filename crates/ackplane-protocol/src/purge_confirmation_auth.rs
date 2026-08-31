//! Domain-separated signatures for authenticated Lifecycle-purge and
//! Recovery-execution transitions.
//!
//! An enrolled signing key proves control of a registered node credential, not
//! the identity of a particular human. The signed operation therefore binds the
//! exact preview or confirmation request so a credential cannot be replayed for
//! a different destructive action. ADR-0145 decision 4 reuses this exact
//! mechanism for Recovery execution rather than inventing a second, competing
//! one -- a distinct domain separator and operation enum, not a parallel copy
//! of `lifecycle_purge_signing_bytes` itself.

use crate::{signing_bytes::push_field, v1};

/// Domain separation for Lifecycle-purge authentication.
pub const LIFECYCLE_PURGE_DOMAIN: &[u8] = b"mindleak.ackplane.v1.lifecycle-purge\0";
/// Domain separation for Recovery-execution authentication (ADR-0145
/// decision 4). Distinct from [`LIFECYCLE_PURGE_DOMAIN`] so a credential
/// signed for one destructive workflow can never verify for the other, even
/// though both reuse the identical preview/confirm signing shape.
pub const RECOVERY_EXECUTION_DOMAIN: &[u8] = b"mindleak.ackplane.v1.recovery-execution\0";

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

/// The exact Recovery-execution operation an enrolled credential authorizes
/// (ADR-0145 decision 4). `Preview` binds the explicit impact plan decision 5
/// requires -- the artifact and its digest, the caller-chosen idempotency
/// key identifying the safety Snapshot to trigger, and the rehearsal relied
/// on -- so a credential cannot be replayed to preview a different restore.
/// The safety Snapshot's own receipt id and digest are not signed here: they
/// are decided only once the server actually executes it, which the caller
/// cannot know in advance -- the signature instead commits to *which*
/// idempotency key the caller chose for it, which is what prevents a
/// signature meant for one safety Snapshot from being replayed against a
/// preview naming a different one. `Confirm` binds only the request id,
/// exactly like `LifecyclePurgeOperation::Confirm`: the request itself
/// already fixed every other field at preview time.
#[derive(Debug, Clone, Copy)]
pub enum RecoveryExecutionOperation<'a> {
    Preview {
        artifact_request_id: &'a str,
        manifest_digest: &'a [u8],
        safety_snapshot_idempotency_key: &'a str,
        rehearsal_id: &'a str,
        confirmation_window_seconds: u64,
        idempotency_key: &'a str,
    },
    Confirm {
        request_id: &'a str,
    },
}

impl RecoveryExecutionOperation<'_> {
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
                artifact_request_id,
                manifest_digest,
                safety_snapshot_idempotency_key,
                rehearsal_id,
                confirmation_window_seconds,
                idempotency_key,
            } => {
                push_field(bytes, artifact_request_id.as_bytes());
                push_field(bytes, manifest_digest);
                push_field(bytes, safety_snapshot_idempotency_key.as_bytes());
                push_field(bytes, rehearsal_id.as_bytes());
                push_field(bytes, &confirmation_window_seconds.to_be_bytes());
                push_field(bytes, idempotency_key.as_bytes());
            }
            Self::Confirm { request_id } => push_field(bytes, request_id.as_bytes()),
        }
    }
}

/// Binds an enrolled key's authentication to one Recovery-execution operation
/// and all of that operation's security-relevant fields, exactly mirroring
/// [`lifecycle_purge_signing_bytes`] with [`RECOVERY_EXECUTION_DOMAIN`] in
/// place of [`LIFECYCLE_PURGE_DOMAIN`] -- the domain separator is what keeps
/// the two workflows' signatures from ever verifying for one another.
pub fn recovery_execution_signing_bytes(
    tenant_id: &str,
    repository_id: &str,
    operation: &RecoveryExecutionOperation<'_>,
    authentication: &v1::ClaimAuthentication,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(RECOVERY_EXECUTION_DOMAIN.len() + 128);
    bytes.extend_from_slice(RECOVERY_EXECUTION_DOMAIN);
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

    fn recovery_preview() -> RecoveryExecutionOperation<'static> {
        RecoveryExecutionOperation::Preview {
            artifact_request_id: "artifact-a",
            manifest_digest: b"01234567890123456789012345678901",
            safety_snapshot_idempotency_key: "safety-snapshot-a",
            rehearsal_id: "rehearsal-a",
            confirmation_window_seconds: 60,
            idempotency_key: "recovery-preview-a",
        }
    }

    #[test]
    fn recovery_preview_signatures_bind_each_mutable_operation_field() {
        let authentication = authentication();
        let preview = recovery_preview();
        let changed = RecoveryExecutionOperation::Preview {
            artifact_request_id: "artifact-a",
            manifest_digest: b"01234567890123456789012345678901",
            safety_snapshot_idempotency_key: "safety-snapshot-a",
            rehearsal_id: "rehearsal-b",
            confirmation_window_seconds: 60,
            idempotency_key: "recovery-preview-a",
        };

        assert_ne!(
            recovery_execution_signing_bytes("tenant-a", "repository-a", &preview, &authentication),
            recovery_execution_signing_bytes("tenant-a", "repository-a", &changed, &authentication)
        );
    }

    #[test]
    fn recovery_preview_and_confirmation_never_share_a_signature_domain() {
        let authentication = authentication();
        let preview = recovery_preview();
        let confirm = RecoveryExecutionOperation::Confirm {
            request_id: "request-a",
        };

        assert_ne!(
            recovery_execution_signing_bytes("tenant-a", "repository-a", &preview, &authentication),
            recovery_execution_signing_bytes("tenant-a", "repository-a", &confirm, &authentication)
        );
    }

    /// ADR-0145 decision 4's entire premise: a domain-separated signature for
    /// one destructive workflow must never verify for the other, even though
    /// both reuse the identical preview/confirm shape and helper.
    #[test]
    fn lifecycle_purge_and_recovery_execution_never_share_a_signature_domain() {
        let authentication = authentication();
        let purge_confirm = LifecyclePurgeOperation::Confirm {
            request_id: "request-a",
        };
        let recovery_confirm = RecoveryExecutionOperation::Confirm {
            request_id: "request-a",
        };

        assert_ne!(
            lifecycle_purge_signing_bytes(
                "tenant-a",
                "repository-a",
                &purge_confirm,
                &authentication
            ),
            recovery_execution_signing_bytes(
                "tenant-a",
                "repository-a",
                &recovery_confirm,
                &authentication
            )
        );
    }
}

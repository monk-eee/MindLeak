//! Canonical payload digests for ADR-0115 grant and revocation events.

use std::time::SystemTime;

use sha2::{Digest, Sha256};

use super::model::{action_codes, DelegationGrantRequest, DelegationRevocationRequest};

pub(super) fn grant_payload_digest(request: &DelegationGrantRequest) -> Vec<u8> {
    let mut hasher = Sha256::new();
    push_field(&mut hasher, b"mindleak.ackplane.v1.delegation.grant\0");
    for field in [
        request.tenant_id.as_bytes(),
        request.repository_id.as_bytes(),
        request.verified_issuer_principal_id.as_bytes(),
        request.delegatee_session_id.as_bytes(),
        request.project_id.as_deref().unwrap_or_default().as_bytes(),
        request.task_id.as_deref().unwrap_or_default().as_bytes(),
        request.goal_id.as_bytes(),
        request.goal_digest.as_slice(),
        request.policy_version.as_bytes(),
        request.policy_digest.as_slice(),
        request.constitution_version.as_bytes(),
        request.constitution_digest.as_slice(),
    ] {
        push_field(&mut hasher, field);
    }
    let actions = action_codes(&request.allowed_actions).expect("validated grant actions");
    hasher.update((actions.len() as u32).to_be_bytes());
    for action in actions {
        hasher.update(action.to_be_bytes());
    }
    hasher.update(request.max_token_budget.to_be_bytes());
    hasher.update(request.max_actions_per_session.to_be_bytes());
    hasher.update(request.source_protocol_version.to_be_bytes());
    push_field(&mut hasher, &unix_nanos(request.effective_at).to_be_bytes());
    push_field(&mut hasher, &unix_nanos(request.expires_at).to_be_bytes());
    hasher.finalize().to_vec()
}

pub(super) fn revocation_payload_digest(request: &DelegationRevocationRequest) -> Vec<u8> {
    let mut hasher = Sha256::new();
    push_field(&mut hasher, b"mindleak.ackplane.v1.delegation.revoke\0");
    for field in [
        request.tenant_id.as_bytes(),
        request.repository_id.as_bytes(),
        request.delegation_id.as_bytes(),
        request.verified_revoker_principal_id.as_bytes(),
        request.reason.as_bytes(),
    ] {
        push_field(&mut hasher, field);
    }
    hasher.update(request.expected_version.to_be_bytes());
    hasher.finalize().to_vec()
}

fn push_field(hasher: &mut Sha256, field: &[u8]) {
    hasher.update((field.len() as u32).to_be_bytes());
    hasher.update(field);
}

fn unix_nanos(timestamp: SystemTime) -> u128 {
    timestamp
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("validated delegation timestamps must be after the Unix epoch")
        .as_nanos()
}

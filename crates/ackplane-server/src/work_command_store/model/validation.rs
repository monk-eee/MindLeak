use super::{
    NewWorkCommand, NewWorkCommandReceipt, WorkCommandKind, WorkCommandStoreError,
    MAX_IDENTIFIER_BYTES, MAX_POLICY_REFS, MAX_RATIONALE_BYTES, MAX_REASON_BYTES,
};
use std::time::SystemTime;

pub(in crate::work_command_store) fn validate_request(
    request: &NewWorkCommand,
    now: SystemTime,
) -> Result<(), WorkCommandStoreError> {
    for (field, value) in [
        ("tenant_id", request.tenant_id.as_str()),
        ("repository_id", request.repository_id.as_str()),
        ("schema_version", request.schema_version.as_str()),
        (
            "issuing_principal_id",
            request.issuing_principal_id.as_str(),
        ),
        ("idempotency_key", request.idempotency_key.as_str()),
    ] {
        require_identifier(field, value)?;
    }
    for (field, value) in [
        ("task_id", request.task_id.as_deref()),
        ("delegation_id", request.delegation_id.as_deref()),
        ("confirmation_id", request.confirmation_id.as_deref()),
    ] {
        validate_optional_identifier(field, value)?;
    }
    if request.policy_refs.len() > MAX_POLICY_REFS
        || request
            .policy_refs
            .iter()
            .any(|reference| !is_identifier(reference))
    {
        return Err(WorkCommandStoreError::InvalidPolicyReferences);
    }
    if request.rationale.is_empty() || request.rationale.len() > MAX_RATIONALE_BYTES {
        return Err(WorkCommandStoreError::InvalidRationale);
    }
    if request.payload_digest.len() != super::DIGEST_BYTES {
        return Err(WorkCommandStoreError::InvalidPayloadDigest);
    }
    if request
        .expected_task_version
        .is_some_and(|version| version < 0)
    {
        return Err(WorkCommandStoreError::InvalidExpectedTaskVersion);
    }
    match request.kind {
        WorkCommandKind::CreateWork
            if request.task_id.is_some() || request.expected_task_version.is_some() =>
        {
            return Err(WorkCommandStoreError::InvalidCreateWorkTarget);
        }
        WorkCommandKind::CreateWork => {}
        _ if request.task_id.is_none() || request.expected_task_version.is_none() => {
            return Err(WorkCommandStoreError::MissingExistingTaskVersion);
        }
        _ => {}
    }
    if request.expires_at <= now {
        return Err(WorkCommandStoreError::InvalidExpiry);
    }
    Ok(())
}

pub(in crate::work_command_store) fn validate_receipt(
    receipt: &NewWorkCommandReceipt,
    now: SystemTime,
) -> Result<(), WorkCommandStoreError> {
    for (field, value) in [
        ("tenant_id", receipt.tenant_id.as_str()),
        ("repository_id", receipt.repository_id.as_str()),
        ("command_id", receipt.command_id.as_str()),
        ("receipt_id", receipt.receipt_id.as_str()),
    ] {
        require_identifier(field, value)?;
    }
    if receipt.reason.len() > MAX_REASON_BYTES {
        return Err(WorkCommandStoreError::InvalidReason);
    }
    if receipt.evidence_refs.len() > MAX_POLICY_REFS
        || receipt
            .evidence_refs
            .iter()
            .any(|reference| !is_identifier(reference))
    {
        return Err(WorkCommandStoreError::InvalidPolicyReferences);
    }
    if receipt.occurred_at > now {
        return Err(WorkCommandStoreError::InvalidReceiptTime);
    }
    Ok(())
}

fn require_identifier(field: &'static str, value: &str) -> Result<(), WorkCommandStoreError> {
    if is_identifier(value) {
        Ok(())
    } else {
        Err(WorkCommandStoreError::InvalidIdentifier { field })
    }
}

fn validate_optional_identifier(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), WorkCommandStoreError> {
    if value.is_none_or(is_identifier) {
        Ok(())
    } else {
        Err(WorkCommandStoreError::InvalidOptionalIdentifier { field })
    }
}

fn is_identifier(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_IDENTIFIER_BYTES
}

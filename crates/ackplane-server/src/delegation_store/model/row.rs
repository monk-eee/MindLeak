//! Decoding and integrity checks for stored delegation event/projection rows.

use std::time::SystemTime;

use tokio_postgres::Row;

use super::{
    actions_from_codes, DelegationEvent, DelegationEventKind, DelegationEventPayload,
    DelegationGrantPayload, DelegationProjection, DelegationProjectionStatus, DelegationStoreError,
    MAX_IDENTIFIER_BYTES, MAX_REASON_BYTES, SHA256_DIGEST_BYTES,
};

pub(crate) fn row_to_event(row: &Row) -> Result<DelegationEvent, DelegationStoreError> {
    let kind_value: i16 = row.get("event_kind");
    let kind = DelegationEventKind::from_i16(kind_value)
        .ok_or(DelegationStoreError::InvalidStoredEventKind(kind_value))?;
    let payload = match kind {
        DelegationEventKind::Granted => {
            let payload = DelegationGrantPayload {
                delegatee_session_id: required(row.get("delegatee_session_id"))?,
                project_id: row.get("project_id"),
                task_id: row.get("task_id"),
                goal_id: required(row.get("goal_id"))?,
                goal_digest: required(row.get("goal_digest"))?,
                policy_version: required(row.get("policy_version"))?,
                policy_digest: required(row.get("policy_digest"))?,
                constitution_version: required(row.get("constitution_version"))?,
                constitution_digest: required(row.get("constitution_digest"))?,
                allowed_actions: actions_from_codes(required(row.get("allowed_actions"))?)?,
                max_token_budget: read_u32_from_i64(
                    required(row.get("max_token_budget"))?,
                    "max_token_budget",
                )?,
                max_actions_per_session: read_u32_from_i64(
                    required(row.get("max_actions_per_session"))?,
                    "max_actions_per_session",
                )?,
                source_protocol_version: read_u16(
                    required(row.get("source_protocol_version"))?,
                    "source_protocol_version",
                )?,
                effective_at: required(row.get("effective_at"))?,
                expires_at: required(row.get("expires_at"))?,
            };
            validate_stored_grant_payload(&payload)?;
            DelegationEventPayload::Granted(Box::new(payload))
        }
        DelegationEventKind::Revoked => {
            let reason: String = required(row.get("revocation_reason"))?;
            if reason.trim().is_empty() || reason.len() > MAX_REASON_BYTES {
                return Err(DelegationStoreError::InvalidStoredPayload);
            }
            DelegationEventPayload::Revoked { reason }
        }
    };
    Ok(DelegationEvent {
        delegation_id: row.get("delegation_id"),
        stream_position: read_u64(row.get("stream_position"), "stream_position")?,
        kind,
        actor_principal_id: row.get("actor_principal_id"),
        expected_prior_version: read_u32_from_i32(
            row.get("expected_prior_version"),
            "expected_prior_version",
        )?,
        resulting_version: read_u32_from_i32(row.get("resulting_version"), "resulting_version")?,
        idempotency_key: row.get("idempotency_key"),
        payload_digest: row.get("payload_digest"),
        schema_version: read_u16(row.get("schema_version"), "schema_version")?,
        recorded_at: row.get("recorded_at"),
        payload,
    })
}

pub(crate) fn row_to_projection(row: &Row) -> Result<DelegationProjection, DelegationStoreError> {
    let status_value: i16 = row.get("status");
    let status = DelegationProjectionStatus::from_i16(status_value)
        .ok_or(DelegationStoreError::InvalidStoredStatus(status_value))?;
    Ok(DelegationProjection {
        delegation_id: row.get("delegation_id"),
        issuer_principal_id: row.get("issuer_principal_id"),
        delegatee_session_id: row.get("delegatee_session_id"),
        project_id: row.get("project_id"),
        task_id: row.get("task_id"),
        goal_id: row.get("goal_id"),
        goal_digest: row.get("goal_digest"),
        policy_version: row.get("policy_version"),
        policy_digest: row.get("policy_digest"),
        constitution_version: row.get("constitution_version"),
        constitution_digest: row.get("constitution_digest"),
        allowed_actions: actions_from_codes(row.get("allowed_actions"))?,
        max_token_budget: read_u32_from_i64(row.get("max_token_budget"), "max_token_budget")?,
        max_actions_per_session: read_u32_from_i64(
            row.get("max_actions_per_session"),
            "max_actions_per_session",
        )?,
        source_protocol_version: read_u16(
            row.get("source_protocol_version"),
            "source_protocol_version",
        )?,
        issued_at: row.get("issued_at"),
        effective_at: row.get("effective_at"),
        expires_at: row.get("expires_at"),
        status,
        version: read_u32_from_i32(row.get("version"), "version")?,
        source_event_position: read_u64(row.get("source_event_position"), "source_event_position")?,
        revoked_at: row.get("revoked_at"),
        revoked_by_principal_id: row.get("revoked_by_principal_id"),
        revocation_reason: row.get("revocation_reason"),
    })
}

pub(crate) fn projection_at_event(
    grant_event: &DelegationEvent,
    event: &DelegationEvent,
) -> Result<DelegationProjection, DelegationStoreError> {
    if grant_event.kind != DelegationEventKind::Granted
        || grant_event.delegation_id != event.delegation_id
    {
        return Err(DelegationStoreError::InvalidStoredPayload);
    }
    let DelegationEventPayload::Granted(grant) = &grant_event.payload else {
        return Err(DelegationStoreError::InvalidStoredPayload);
    };
    let mut projection = DelegationProjection {
        delegation_id: grant_event.delegation_id.clone(),
        issuer_principal_id: grant_event.actor_principal_id.clone(),
        delegatee_session_id: grant.delegatee_session_id.clone(),
        project_id: grant.project_id.clone(),
        task_id: grant.task_id.clone(),
        goal_id: grant.goal_id.clone(),
        goal_digest: grant.goal_digest.clone(),
        policy_version: grant.policy_version.clone(),
        policy_digest: grant.policy_digest.clone(),
        constitution_version: grant.constitution_version.clone(),
        constitution_digest: grant.constitution_digest.clone(),
        allowed_actions: grant.allowed_actions.clone(),
        max_token_budget: grant.max_token_budget,
        max_actions_per_session: grant.max_actions_per_session,
        source_protocol_version: grant.source_protocol_version,
        issued_at: grant_event.recorded_at,
        effective_at: grant.effective_at,
        expires_at: grant.expires_at,
        status: DelegationProjectionStatus::Active,
        version: grant_event.resulting_version,
        source_event_position: grant_event.stream_position,
        revoked_at: None,
        revoked_by_principal_id: None,
        revocation_reason: None,
    };
    if event.kind == DelegationEventKind::Revoked {
        let DelegationEventPayload::Revoked { reason } = &event.payload else {
            return Err(DelegationStoreError::InvalidStoredPayload);
        };
        projection.status = DelegationProjectionStatus::Revoked;
        projection.version = event.resulting_version;
        projection.source_event_position = event.stream_position;
        projection.revoked_at = Some(event.recorded_at);
        projection.revoked_by_principal_id = Some(event.actor_principal_id.clone());
        projection.revocation_reason = Some(reason.clone());
    }
    Ok(projection)
}

fn required<T>(value: Option<T>) -> Result<T, DelegationStoreError> {
    value.ok_or(DelegationStoreError::InvalidStoredPayload)
}

fn validate_stored_grant_payload(
    payload: &DelegationGrantPayload,
) -> Result<(), DelegationStoreError> {
    for value in [
        payload.delegatee_session_id.as_str(),
        payload.goal_id.as_str(),
        payload.policy_version.as_str(),
        payload.constitution_version.as_str(),
    ] {
        if value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES {
            return Err(DelegationStoreError::InvalidStoredPayload);
        }
    }
    for value in [payload.project_id.as_deref(), payload.task_id.as_deref()] {
        if value.is_some_and(|value| value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES)
        {
            return Err(DelegationStoreError::InvalidStoredPayload);
        }
    }
    if payload.goal_digest.len() != SHA256_DIGEST_BYTES
        || payload.policy_digest.len() != SHA256_DIGEST_BYTES
        || payload.constitution_digest.len() != SHA256_DIGEST_BYTES
        || payload.max_token_budget == 0
        || payload.max_actions_per_session == 0
        || payload.source_protocol_version == 0
        || payload.effective_at < SystemTime::UNIX_EPOCH
        || payload.expires_at <= payload.effective_at
    {
        return Err(DelegationStoreError::InvalidStoredPayload);
    }
    Ok(())
}

fn read_u16(value: i16, field: &'static str) -> Result<u16, DelegationStoreError> {
    u16::try_from(value).map_err(|_| DelegationStoreError::InvalidStoredNumber { field })
}

fn read_u32_from_i64(value: i64, field: &'static str) -> Result<u32, DelegationStoreError> {
    u32::try_from(value).map_err(|_| DelegationStoreError::InvalidStoredNumber { field })
}

fn read_u32_from_i32(value: i32, field: &'static str) -> Result<u32, DelegationStoreError> {
    u32::try_from(value).map_err(|_| DelegationStoreError::InvalidStoredNumber { field })
}

fn read_u64(value: i64, field: &'static str) -> Result<u64, DelegationStoreError> {
    u64::try_from(value).map_err(|_| DelegationStoreError::InvalidStoredNumber { field })
}

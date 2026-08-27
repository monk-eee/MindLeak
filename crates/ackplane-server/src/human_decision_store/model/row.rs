//! Decoding and integrity checks for stored human decision event/projection
//! rows.

use tokio_postgres::Row;

use super::{
    HumanDecisionEvent, HumanDecisionEventKind, HumanDecisionEventPayload, HumanDecisionProjection,
    HumanDecisionRequestedPayload, HumanDecisionResolutionOutcome, HumanDecisionStatus,
    HumanDecisionStoreError, SafeBehavior, MAX_IDENTIFIER_BYTES, MAX_REASON_BYTES,
    SHA256_DIGEST_BYTES,
};

pub(crate) fn row_to_event(row: &Row) -> Result<HumanDecisionEvent, HumanDecisionStoreError> {
    let kind_value: i16 = row.get("event_kind");
    let kind = HumanDecisionEventKind::from_i16(kind_value)
        .ok_or(HumanDecisionStoreError::InvalidStoredEventKind(kind_value))?;
    let payload = match kind {
        HumanDecisionEventKind::Requested => {
            let safe_behavior_value: i16 = required(row.get("safe_behavior"))?;
            let payload = HumanDecisionRequestedPayload {
                proposed_action: required(row.get("proposed_action"))?,
                target: required(row.get("target"))?,
                reason: required(row.get("reason"))?,
                context_packet_digest: required(row.get("context_packet_digest"))?,
                evidence_digest: required(row.get("evidence_digest"))?,
                alternatives: required(row.get("alternatives"))?,
                safe_behavior: SafeBehavior::from_i16(safe_behavior_value).ok_or(
                    HumanDecisionStoreError::InvalidStoredSafeBehavior(safe_behavior_value),
                )?,
                related_delegation_id: row.get("related_delegation_id"),
                expires_at: required(row.get("expires_at"))?,
            };
            validate_stored_requested_payload(&payload)?;
            HumanDecisionEventPayload::Requested(Box::new(payload))
        }
        HumanDecisionEventKind::Approved | HumanDecisionEventKind::Denied => {
            let rationale: String = required(row.get("rationale"))?;
            if rationale.trim().is_empty() || rationale.len() > MAX_REASON_BYTES {
                return Err(HumanDecisionStoreError::InvalidStoredPayload);
            }
            let outcome = if kind == HumanDecisionEventKind::Approved {
                HumanDecisionResolutionOutcome::Approved
            } else {
                HumanDecisionResolutionOutcome::Denied
            };
            HumanDecisionEventPayload::Resolved { outcome, rationale }
        }
    };
    Ok(HumanDecisionEvent {
        decision_id: row.get("decision_id"),
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

pub(crate) fn row_to_projection(
    row: &Row,
) -> Result<HumanDecisionProjection, HumanDecisionStoreError> {
    let status_value: i16 = row.get("status");
    let status = HumanDecisionStatus::from_i16(status_value)
        .ok_or(HumanDecisionStoreError::InvalidStoredStatus(status_value))?;
    let safe_behavior_value: i16 = row.get("safe_behavior");
    let safe_behavior = SafeBehavior::from_i16(safe_behavior_value).ok_or(
        HumanDecisionStoreError::InvalidStoredSafeBehavior(safe_behavior_value),
    )?;
    Ok(HumanDecisionProjection {
        decision_id: row.get("decision_id"),
        proposing_principal_id: row.get("proposing_principal_id"),
        proposed_action: row.get("proposed_action"),
        target: row.get("target"),
        reason: row.get("reason"),
        context_packet_digest: row.get("context_packet_digest"),
        evidence_digest: row.get("evidence_digest"),
        alternatives: row.get("alternatives"),
        safe_behavior,
        related_delegation_id: row.get("related_delegation_id"),
        requested_at: row.get("requested_at"),
        expires_at: row.get("expires_at"),
        status,
        version: read_u32_from_i32(row.get("version"), "version")?,
        source_event_position: read_u64(row.get("source_event_position"), "source_event_position")?,
        resolved_at: row.get("resolved_at"),
        resolved_by_principal_id: row.get("resolved_by_principal_id"),
        resolution_rationale: row.get("resolution_rationale"),
    })
}

pub(crate) fn projection_at_event(
    requested_event: &HumanDecisionEvent,
    event: &HumanDecisionEvent,
) -> Result<HumanDecisionProjection, HumanDecisionStoreError> {
    if requested_event.kind != HumanDecisionEventKind::Requested
        || requested_event.decision_id != event.decision_id
    {
        return Err(HumanDecisionStoreError::InvalidStoredPayload);
    }
    let HumanDecisionEventPayload::Requested(requested) = &requested_event.payload else {
        return Err(HumanDecisionStoreError::InvalidStoredPayload);
    };
    let mut projection = HumanDecisionProjection {
        decision_id: requested_event.decision_id.clone(),
        proposing_principal_id: requested_event.actor_principal_id.clone(),
        proposed_action: requested.proposed_action.clone(),
        target: requested.target.clone(),
        reason: requested.reason.clone(),
        context_packet_digest: requested.context_packet_digest.clone(),
        evidence_digest: requested.evidence_digest.clone(),
        alternatives: requested.alternatives.clone(),
        safe_behavior: requested.safe_behavior,
        related_delegation_id: requested.related_delegation_id.clone(),
        requested_at: requested_event.recorded_at,
        expires_at: requested.expires_at,
        status: HumanDecisionStatus::Pending,
        version: requested_event.resulting_version,
        source_event_position: requested_event.stream_position,
        resolved_at: None,
        resolved_by_principal_id: None,
        resolution_rationale: None,
    };
    if event.decision_id != requested_event.decision_id {
        return Err(HumanDecisionStoreError::InvalidStoredPayload);
    }
    match &event.payload {
        HumanDecisionEventPayload::Requested(_) => {}
        HumanDecisionEventPayload::Resolved { outcome, rationale } => {
            projection.status = match outcome {
                HumanDecisionResolutionOutcome::Approved => HumanDecisionStatus::Approved,
                HumanDecisionResolutionOutcome::Denied => HumanDecisionStatus::Denied,
            };
            projection.version = event.resulting_version;
            projection.source_event_position = event.stream_position;
            projection.resolved_at = Some(event.recorded_at);
            projection.resolved_by_principal_id = Some(event.actor_principal_id.clone());
            projection.resolution_rationale = Some(rationale.clone());
        }
    }
    Ok(projection)
}

fn validate_stored_requested_payload(
    payload: &HumanDecisionRequestedPayload,
) -> Result<(), HumanDecisionStoreError> {
    for value in [payload.proposed_action.as_str(), payload.target.as_str()] {
        if value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES {
            return Err(HumanDecisionStoreError::InvalidStoredPayload);
        }
    }
    if payload.reason.trim().is_empty() || payload.reason.len() > MAX_REASON_BYTES {
        return Err(HumanDecisionStoreError::InvalidStoredPayload);
    }
    if payload.alternatives.trim().is_empty() || payload.alternatives.len() > MAX_REASON_BYTES {
        return Err(HumanDecisionStoreError::InvalidStoredPayload);
    }
    for digest in [&payload.context_packet_digest, &payload.evidence_digest] {
        if digest.len() != SHA256_DIGEST_BYTES {
            return Err(HumanDecisionStoreError::InvalidStoredPayload);
        }
    }
    Ok(())
}

fn required<T>(value: Option<T>) -> Result<T, HumanDecisionStoreError> {
    value.ok_or(HumanDecisionStoreError::InvalidStoredPayload)
}

fn read_u64(value: i64, field: &'static str) -> Result<u64, HumanDecisionStoreError> {
    u64::try_from(value).map_err(|_| HumanDecisionStoreError::InvalidStoredNumber { field })
}

fn read_u32_from_i32(value: i32, field: &'static str) -> Result<u32, HumanDecisionStoreError> {
    u32::try_from(value).map_err(|_| HumanDecisionStoreError::InvalidStoredNumber { field })
}

fn read_u16(value: i16, field: &'static str) -> Result<u16, HumanDecisionStoreError> {
    u16::try_from(value).map_err(|_| HumanDecisionStoreError::InvalidStoredNumber { field })
}

//! Repository-stream locking and immutable event replay helpers.

use tokio_postgres::Transaction;

use super::{
    model::{projection_at_event, row_to_event, row_to_projection},
    DelegationEvent, DelegationEventKind, DelegationOutcome, DelegationProjection,
    DelegationStoreError, EVENT_COLUMNS, PROJECTION_COLUMNS,
};

pub(super) async fn lock_stream(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    repository_id: &str,
) -> Result<i64, DelegationStoreError> {
    Ok(transaction
        .query_one(
            "INSERT INTO delegation_stream_heads (tenant_id, repository_id, stream_position) \
             VALUES ($1,$2,0) \
             ON CONFLICT (tenant_id, repository_id) DO UPDATE \
                SET stream_position = delegation_stream_heads.stream_position \
             RETURNING stream_position",
            &[&tenant_id, &repository_id],
        )
        .await?
        .get("stream_position"))
}

pub(super) async fn advance_stream(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    repository_id: &str,
    stream_position: i64,
) -> Result<(), DelegationStoreError> {
    transaction
        .execute(
            "UPDATE delegation_stream_heads SET stream_position = $3 \
             WHERE tenant_id = $1 AND repository_id = $2",
            &[&tenant_id, &repository_id, &stream_position],
        )
        .await?;
    Ok(())
}

pub(super) fn next_stream_position(current_position: i64) -> Result<i64, DelegationStoreError> {
    current_position
        .checked_add(1)
        .filter(|position| *position > 0)
        .ok_or(DelegationStoreError::StreamPositionExhausted)
}

pub(super) async fn grant_event_from_transaction(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    repository_id: &str,
    delegation_id: &str,
) -> Result<DelegationEvent, DelegationStoreError> {
    let row = transaction
        .query_opt(
            &format!(
                "SELECT {EVENT_COLUMNS} FROM delegation_events \
                 WHERE tenant_id = $1 AND repository_id = $2 AND delegation_id = $3 \
                   AND event_kind = $4 \
                 ORDER BY stream_position ASC LIMIT 1"
            ),
            &[
                &tenant_id,
                &repository_id,
                &delegation_id,
                &DelegationEventKind::Granted.as_i16(),
            ],
        )
        .await?
        .ok_or(DelegationStoreError::InvalidStoredPayload)?;
    row_to_event(&row)
}

pub(super) async fn idempotent_outcome(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    repository_id: &str,
    idempotency_key: &str,
    expected_kind: DelegationEventKind,
    expected_payload_digest: &[u8],
) -> Result<Option<DelegationOutcome>, DelegationStoreError> {
    let existing = transaction
        .query_opt(
            &format!(
                "SELECT {EVENT_COLUMNS} FROM delegation_events \
                 WHERE tenant_id = $1 AND repository_id = $2 AND idempotency_key = $3 FOR UPDATE"
            ),
            &[&tenant_id, &repository_id, &idempotency_key],
        )
        .await?;
    let Some(row) = existing else {
        return Ok(None);
    };
    let event = row_to_event(&row)?;
    if event.kind != expected_kind || event.payload_digest != expected_payload_digest {
        return Err(DelegationStoreError::IdempotencyConflict);
    }
    let grant_event = if event.kind == DelegationEventKind::Granted {
        event.clone()
    } else {
        grant_event_from_transaction(transaction, tenant_id, repository_id, &event.delegation_id)
            .await?
    };
    let projection = projection_at_event(&grant_event, &event)?;
    Ok(Some(DelegationOutcome {
        projection,
        event,
        idempotent_replay: true,
    }))
}

pub(super) async fn projection_from_transaction(
    transaction: &Transaction<'_>,
    tenant_id: &str,
    repository_id: &str,
    delegation_id: &str,
    for_update: bool,
) -> Result<Option<DelegationProjection>, DelegationStoreError> {
    let lock = if for_update { " FOR UPDATE" } else { "" };
    transaction
        .query_opt(
            &format!(
                "SELECT {PROJECTION_COLUMNS} FROM delegation_projections \
                 WHERE tenant_id = $1 AND repository_id = $2 AND delegation_id = $3{lock}"
            ),
            &[&tenant_id, &repository_id, &delegation_id],
        )
        .await?
        .map(|row| row_to_projection(&row))
        .transpose()
}

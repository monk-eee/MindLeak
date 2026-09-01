//! Value objects, validation, and row decoding for supervisor projections.

use std::time::{SystemTime, UNIX_EPOCH};

use ackplane_protocol::supervisor::{
    SupervisorCapabilities, SupervisorError, SupervisorIdentity, SupervisorLifecycleReason,
    SupervisorLifecycleReceipt, SupervisorRegistration, SupervisorRuntime, SupervisorSession,
    SupervisorWorkerState,
};
use sha2::{Digest, Sha256};
use tokio_postgres::Row;

pub const HEARTBEAT_STALE_AFTER_SECS: i64 = 90;
const MAX_IDENTIFIER_BYTES: usize = 256;
const MAX_IDEMPOTENCY_KEY_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorLifecycleReceiptRequest {
    pub tenant_id: String,
    pub repository_id: String,
    pub receipt: SupervisorLifecycleReceipt,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorFreshness {
    NeverReported,
    Current,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorStatus {
    pub registration: SupervisorRegistration,
    pub registered_at: SystemTime,
    pub last_heartbeat_at: Option<i64>,
    pub freshness: SupervisorFreshness,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorRegistrationOutcome {
    pub status: SupervisorStatus,
    pub idempotent_replay: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorSessionProjection {
    pub session: SupervisorSession,
    pub current_reason: Option<SupervisorLifecycleReason>,
    pub current_occurred_at: i64,
    pub recorded_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorSessionOutcome {
    pub projection: SupervisorSessionProjection,
    pub idempotent_replay: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorLifecycleReceiptRecord {
    pub receipt_position: i64,
    pub receipt: SupervisorLifecycleReceipt,
    pub idempotency_key: String,
    pub recorded_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorLifecycleOutcome {
    pub receipt: SupervisorLifecycleReceiptRecord,
    pub projection: SupervisorSessionProjection,
    pub idempotent_replay: bool,
    pub projection_advanced: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum SupervisorStoreError {
    #[error("supervisor database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("supervisor store could not obtain a database connection: {0}")]
    PoolExhausted(#[from] deadpool_postgres::PoolError),
    #[error("invalid supervisor contract: {0}")]
    Contract(#[from] SupervisorError),
    #[error("supervisor JSON serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("{field} must be a bounded non-empty identifier")]
    InvalidIdentifier { field: &'static str },
    #[error("{field} must be a non-negative Unix second")]
    InvalidTimestamp { field: &'static str },
    #[error("idempotency_key must be a bounded non-empty identifier")]
    InvalidIdempotencyKey,
    #[error("heartbeat freshness must be positive")]
    InvalidHeartbeatFreshness,
    #[error("the server clock predates the Unix epoch or exceeds the supported range")]
    InvalidServerClock,
    #[error("supervisor registration already exists with different content")]
    RegistrationConflict,
    #[error("supervisor session already exists with different content")]
    SessionConflict,
    #[error("supervisor is not registered in this tenant and repository")]
    UnknownSupervisor,
    #[error("supervisor session is not registered in this tenant and repository")]
    UnknownSession,
    #[error("lifecycle receipt does not match the stored supervisor/session identity")]
    SessionIdentityMismatch,
    #[error("lifecycle receipt predates its stored supervisor session")]
    ReceiptBeforeSession,
    #[error("idempotency key was already used for a different lifecycle receipt")]
    IdempotencyConflict,
    #[error("stored supervisor capabilities are invalid: {detail}")]
    InvalidStoredCapabilities { detail: String },
    #[error("stored {field} value {value} is invalid")]
    InvalidStoredEnum { field: &'static str, value: i16 },
    #[error("supervisor outbox sequence {sequence} exceeds the supported range")]
    OutboxSequenceOutOfRange { sequence: u64 },
}

pub(super) fn registration_values(
    registration: &SupervisorRegistration,
) -> Result<(String, Vec<u8>), SupervisorStoreError> {
    registration.validate()?;
    validate_identity(&registration.identity)?;
    require_identifier("supervisor_id", &registration.supervisor_id)?;
    require_identifier("supervisor_version", &registration.supervisor_version)?;
    require_identifier("protocol_version", &registration.protocol_version)?;
    let capabilities = serde_json::to_string(&registration.capabilities)?;
    if capabilities.len() < 2 || capabilities.len() > 4096 {
        return Err(SupervisorStoreError::InvalidStoredCapabilities {
            detail: "serialized capability declaration exceeds its bounded representation"
                .to_string(),
        });
    }
    Ok((
        capabilities,
        Sha256::digest(serde_json::to_vec(registration)?).to_vec(),
    ))
}

pub(super) fn session_digest(
    tenant_id: &str,
    repository_id: &str,
    session: &SupervisorSession,
) -> Result<Vec<u8>, SupervisorStoreError> {
    validate_scope(tenant_id, repository_id)?;
    session.validate()?;
    if session.started_at < 0 {
        return Err(SupervisorStoreError::InvalidTimestamp {
            field: "started_at",
        });
    }
    Ok(Sha256::digest(serde_json::to_vec(session)?).to_vec())
}

pub(super) fn receipt_digest(
    request: &SupervisorLifecycleReceiptRequest,
) -> Result<Vec<u8>, SupervisorStoreError> {
    validate_scope(&request.tenant_id, &request.repository_id)?;
    request.receipt.validate()?;
    if request.receipt.occurred_at < 0 {
        return Err(SupervisorStoreError::InvalidTimestamp {
            field: "occurred_at",
        });
    }
    if request.idempotency_key.trim().is_empty()
        || request.idempotency_key.len() > MAX_IDEMPOTENCY_KEY_BYTES
    {
        return Err(SupervisorStoreError::InvalidIdempotencyKey);
    }
    Ok(Sha256::digest(serde_json::to_vec(&request.receipt)?).to_vec())
}

pub(super) fn validate_heartbeat(
    tenant_id: &str,
    repository_id: &str,
    supervisor_id: &str,
    observed_at: i64,
) -> Result<(), SupervisorStoreError> {
    validate_scope(tenant_id, repository_id)?;
    require_identifier("supervisor_id", supervisor_id)?;
    if observed_at < 0 {
        return Err(SupervisorStoreError::InvalidTimestamp {
            field: "observed_at",
        });
    }
    Ok(())
}

pub(super) fn validate_scope(
    tenant_id: &str,
    repository_id: &str,
) -> Result<(), SupervisorStoreError> {
    require_identifier("tenant_id", tenant_id)?;
    require_identifier("repository_id", repository_id)
}

pub(super) fn server_now_seconds() -> Result<i64, SupervisorStoreError> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| SupervisorStoreError::InvalidServerClock)?;
    i64::try_from(elapsed.as_secs()).map_err(|_| SupervisorStoreError::InvalidServerClock)
}

pub(super) fn registration_status_from_row(
    row: &Row,
    now_seconds: i64,
    stale_after_secs: i64,
) -> Result<SupervisorStatus, SupervisorStoreError> {
    if stale_after_secs <= 0 {
        return Err(SupervisorStoreError::InvalidHeartbeatFreshness);
    }
    let capabilities_json: String = row.get("capabilities");
    let capabilities: SupervisorCapabilities =
        serde_json::from_str(&capabilities_json).map_err(|error| {
            SupervisorStoreError::InvalidStoredCapabilities {
                detail: error.to_string(),
            }
        })?;
    capabilities.validate()?;
    let registration = SupervisorRegistration {
        supervisor_id: row.get("supervisor_id"),
        identity: SupervisorIdentity {
            tenant_id: row.get("tenant_id"),
            repository_id: row.get("repository_id"),
            node_id: row.get("node_id"),
        },
        supervisor_version: row.get("supervisor_version"),
        protocol_version: row.get("protocol_version"),
        capabilities,
    };
    registration.validate()?;
    let last_heartbeat_at: Option<i64> = row.get("last_heartbeat_at");
    let freshness = match last_heartbeat_at {
        None => SupervisorFreshness::NeverReported,
        Some(observed_at) if now_seconds.saturating_sub(observed_at) <= stale_after_secs => {
            SupervisorFreshness::Current
        }
        Some(_) => SupervisorFreshness::Stale,
    };
    Ok(SupervisorStatus {
        registration,
        registered_at: row.get("registered_at"),
        last_heartbeat_at,
        freshness,
    })
}

pub(super) fn session_projection_from_row(
    row: &Row,
) -> Result<SupervisorSessionProjection, SupervisorStoreError> {
    let state = state_from_code(row.get("current_state"))?;
    let current_reason = row
        .get::<_, Option<i16>>("current_reason")
        .map(reason_from_code)
        .transpose()?;
    let projection = SupervisorSessionProjection {
        session: SupervisorSession {
            session_id: row.get("session_id"),
            supervisor_id: row.get("supervisor_id"),
            worker_id: row.get("worker_id"),
            runtime: runtime_from_code(row.get("runtime"))?,
            started_at: row.get("started_at"),
            state,
        },
        current_reason,
        current_occurred_at: row.get("current_occurred_at"),
        recorded_at: row.get("recorded_at"),
    };
    projection.session.validate()?;
    Ok(projection)
}

pub(super) fn receipt_record_from_row(
    row: &Row,
) -> Result<SupervisorLifecycleReceiptRecord, SupervisorStoreError> {
    let receipt = SupervisorLifecycleReceipt {
        supervisor_id: row.get("supervisor_id"),
        session_id: row.get("session_id"),
        worker_id: row.get("worker_id"),
        occurred_at: row.get("occurred_at"),
        state: state_from_code(row.get("state"))?,
        reason: row
            .get::<_, Option<i16>>("reason")
            .map(reason_from_code)
            .transpose()?,
    };
    receipt.validate()?;
    Ok(SupervisorLifecycleReceiptRecord {
        receipt_position: row.get("receipt_position"),
        receipt,
        idempotency_key: row.get("idempotency_key"),
        recorded_at: row.get("recorded_at"),
    })
}

pub(super) fn runtime_code(runtime: SupervisorRuntime) -> i16 {
    match runtime {
        SupervisorRuntime::LocalMachine => 1,
        SupervisorRuntime::CloudWorker => 2,
        SupervisorRuntime::Pipeline => 3,
        SupervisorRuntime::Service => 4,
    }
}

pub(super) fn state_code(state: SupervisorWorkerState) -> i16 {
    match state {
        SupervisorWorkerState::Started => 1,
        SupervisorWorkerState::Checkpointed => 2,
        SupervisorWorkerState::Paused => 3,
        SupervisorWorkerState::Draining => 4,
        SupervisorWorkerState::Terminated => 5,
        SupervisorWorkerState::Failed => 6,
        SupervisorWorkerState::Disconnected => 7,
        SupervisorWorkerState::Reconnected => 8,
        SupervisorWorkerState::Completed => 9,
    }
}

pub(super) fn reason_code(reason: SupervisorLifecycleReason) -> i16 {
    match reason {
        SupervisorLifecycleReason::CapabilityMissing => 1,
        SupervisorLifecycleReason::CheckpointFailed => 2,
        SupervisorLifecycleReason::DirectiveExpired => 3,
        SupervisorLifecycleReason::OutboxUnavailable => 4,
        SupervisorLifecycleReason::ProtocolUnsupported => 5,
        SupervisorLifecycleReason::SupervisorFailed => 6,
        SupervisorLifecycleReason::WorkerLost => 7,
    }
}

fn runtime_from_code(value: i16) -> Result<SupervisorRuntime, SupervisorStoreError> {
    match value {
        1 => Ok(SupervisorRuntime::LocalMachine),
        2 => Ok(SupervisorRuntime::CloudWorker),
        3 => Ok(SupervisorRuntime::Pipeline),
        4 => Ok(SupervisorRuntime::Service),
        _ => Err(SupervisorStoreError::InvalidStoredEnum {
            field: "runtime",
            value,
        }),
    }
}

fn state_from_code(value: i16) -> Result<SupervisorWorkerState, SupervisorStoreError> {
    match value {
        1 => Ok(SupervisorWorkerState::Started),
        2 => Ok(SupervisorWorkerState::Checkpointed),
        3 => Ok(SupervisorWorkerState::Paused),
        4 => Ok(SupervisorWorkerState::Draining),
        5 => Ok(SupervisorWorkerState::Terminated),
        6 => Ok(SupervisorWorkerState::Failed),
        7 => Ok(SupervisorWorkerState::Disconnected),
        8 => Ok(SupervisorWorkerState::Reconnected),
        9 => Ok(SupervisorWorkerState::Completed),
        _ => Err(SupervisorStoreError::InvalidStoredEnum {
            field: "state",
            value,
        }),
    }
}

fn reason_from_code(value: i16) -> Result<SupervisorLifecycleReason, SupervisorStoreError> {
    match value {
        1 => Ok(SupervisorLifecycleReason::CapabilityMissing),
        2 => Ok(SupervisorLifecycleReason::CheckpointFailed),
        3 => Ok(SupervisorLifecycleReason::DirectiveExpired),
        4 => Ok(SupervisorLifecycleReason::OutboxUnavailable),
        5 => Ok(SupervisorLifecycleReason::ProtocolUnsupported),
        6 => Ok(SupervisorLifecycleReason::SupervisorFailed),
        7 => Ok(SupervisorLifecycleReason::WorkerLost),
        _ => Err(SupervisorStoreError::InvalidStoredEnum {
            field: "reason",
            value,
        }),
    }
}

fn validate_identity(identity: &SupervisorIdentity) -> Result<(), SupervisorStoreError> {
    for (field, value) in [
        ("tenant_id", identity.tenant_id.as_str()),
        ("repository_id", identity.repository_id.as_str()),
        ("node_id", identity.node_id.as_str()),
    ] {
        require_identifier(field, value)?;
    }
    Ok(())
}

pub(super) fn require_identifier(
    field: &'static str,
    value: &str,
) -> Result<(), SupervisorStoreError> {
    if value.trim().is_empty() || value.len() > MAX_IDENTIFIER_BYTES {
        return Err(SupervisorStoreError::InvalidIdentifier { field });
    }
    Ok(())
}

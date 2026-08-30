//! Closed Live Feed vocabulary, bounded values, and row decoding.

use std::time::SystemTime;

use tokio_postgres::Row;

pub(super) const SHA256_DIGEST_BYTES: usize = 32;
const MAX_TENANT_BYTES: usize = 256;
const MAX_REPOSITORY_BYTES: usize = 256;
const MAX_RESOURCE_ID_BYTES: usize = 256;
const MAX_IDENTITY_BYTES: usize = 256;
const MAX_CURSOR_BYTES: usize = 128;
const MAX_REPLAY_LIMIT: i64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveFeedEventKind {
    RepositoryPresence,
    TaskLease,
    WaitRequest,
    ContextPacket,
    KnowledgeLifecycle,
    EvidenceConformance,
    DirectiveReceipt,
    ProjectionFreshness,
    TelemetryHealth,
}

impl LiveFeedEventKind {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::RepositoryPresence => 1,
            Self::TaskLease => 2,
            Self::WaitRequest => 3,
            Self::ContextPacket => 4,
            Self::KnowledgeLifecycle => 5,
            Self::EvidenceConformance => 6,
            Self::DirectiveReceipt => 7,
            Self::ProjectionFreshness => 8,
            Self::TelemetryHealth => 9,
        }
    }

    fn from_i16(value: i16) -> Option<Self> {
        match value {
            1 => Some(Self::RepositoryPresence),
            2 => Some(Self::TaskLease),
            3 => Some(Self::WaitRequest),
            4 => Some(Self::ContextPacket),
            5 => Some(Self::KnowledgeLifecycle),
            6 => Some(Self::EvidenceConformance),
            7 => Some(Self::DirectiveReceipt),
            8 => Some(Self::ProjectionFreshness),
            9 => Some(Self::TelemetryHealth),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveFeedResourceType {
    Repository,
    Node,
    Task,
    Wait,
    ContextPacket,
    Knowledge,
    Evidence,
    Directive,
    Telemetry,
}

impl LiveFeedResourceType {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::Repository => 1,
            Self::Node => 2,
            Self::Task => 3,
            Self::Wait => 4,
            Self::ContextPacket => 5,
            Self::Knowledge => 6,
            Self::Evidence => 7,
            Self::Directive => 8,
            Self::Telemetry => 9,
        }
    }

    fn from_i16(value: i16) -> Option<Self> {
        match value {
            1 => Some(Self::Repository),
            2 => Some(Self::Node),
            3 => Some(Self::Task),
            4 => Some(Self::Wait),
            5 => Some(Self::ContextPacket),
            6 => Some(Self::Knowledge),
            7 => Some(Self::Evidence),
            8 => Some(Self::Directive),
            9 => Some(Self::Telemetry),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveFeedChangeKind {
    Available,
    Updated,
    Invalidated,
    Healthy,
    AttentionRequired,
    Failed,
}

impl LiveFeedChangeKind {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::Available => 1,
            Self::Updated => 2,
            Self::Invalidated => 3,
            Self::Healthy => 4,
            Self::AttentionRequired => 5,
            Self::Failed => 6,
        }
    }

    fn from_i16(value: i16) -> Option<Self> {
        match value {
            1 => Some(Self::Available),
            2 => Some(Self::Updated),
            3 => Some(Self::Invalidated),
            4 => Some(Self::Healthy),
            5 => Some(Self::AttentionRequired),
            6 => Some(Self::Failed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveFeedProjectionFreshness {
    Current,
    Lagging,
    Unavailable,
}

impl LiveFeedProjectionFreshness {
    pub(super) fn as_i16(self) -> i16 {
        match self {
            Self::Current => 1,
            Self::Lagging => 2,
            Self::Unavailable => 3,
        }
    }

    fn from_i16(value: i16) -> Option<Self> {
        match value {
            1 => Some(Self::Current),
            2 => Some(Self::Lagging),
            3 => Some(Self::Unavailable),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveFeedEventPublication {
    pub tenant_id: String,
    pub repository_id: Option<String>,
    pub event_kind: LiveFeedEventKind,
    pub resource_type: LiveFeedResourceType,
    pub resource_id: String,
    pub change_kind: LiveFeedChangeKind,
    pub resource_version: Option<i64>,
    pub source_ledger_position: Option<i64>,
    pub projection_position: Option<i64>,
    pub projection_freshness: Option<LiveFeedProjectionFreshness>,
    pub snapshot_reload: bool,
    pub source_digest: Vec<u8>,
    pub published_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveFeedEvent {
    pub cursor: String,
    pub tenant_id: String,
    pub repository_id: Option<String>,
    pub event_kind: LiveFeedEventKind,
    pub resource_type: LiveFeedResourceType,
    pub resource_id: String,
    pub change_kind: LiveFeedChangeKind,
    pub resource_version: Option<i64>,
    pub source_ledger_position: Option<i64>,
    pub projection_position: Option<i64>,
    pub projection_freshness: Option<LiveFeedProjectionFreshness>,
    pub snapshot_reload: bool,
    pub source_digest: Vec<u8>,
    pub published_by: String,
    pub emitted_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveFeedReplay {
    Events {
        events: Vec<LiveFeedEvent>,
        latest_cursor: Option<String>,
    },
    ResyncRequired {
        reason: LiveFeedResyncReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveFeedResyncReason {
    UnknownCursor,
}

#[derive(Debug, thiserror::Error)]
pub enum LiveFeedStoreError {
    #[error("live feed database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    #[error("no PostgreSQL connection became available within the pool timeout: {0}")]
    PoolExhausted(#[from] deadpool_postgres::PoolError),
    #[error("tenant_id must be between 1 and {MAX_TENANT_BYTES} bytes")]
    InvalidTenant,
    #[error("repository_id must be between 1 and {MAX_REPOSITORY_BYTES} bytes when supplied")]
    InvalidRepository,
    #[error("resource_id must be between 1 and {MAX_RESOURCE_ID_BYTES} bytes")]
    InvalidResource,
    #[error("published_by must be between 1 and {MAX_IDENTITY_BYTES} bytes")]
    InvalidPublisher,
    #[error("source_digest must be exactly {SHA256_DIGEST_BYTES} bytes")]
    InvalidSourceDigest,
    #[error("cursor must be between 1 and {MAX_CURSOR_BYTES} bytes")]
    InvalidCursor,
    #[error("limit must be between 1 and {MAX_REPLAY_LIMIT}")]
    InvalidLimit,
    #[error("the OS random source failed: {0}")]
    Random(String),
    #[error("stored live feed event kind {0} is outside the contract")]
    UnknownEventKind(i16),
    #[error("stored live feed resource type {0} is outside the contract")]
    UnknownResourceType(i16),
    #[error("stored live feed change kind {0} is outside the contract")]
    UnknownChangeKind(i16),
    #[error("stored live feed projection freshness {0} is outside the contract")]
    UnknownProjectionFreshness(i16),
}

pub(super) fn validate_publication(
    publication: &LiveFeedEventPublication,
) -> Result<(), LiveFeedStoreError> {
    if !is_bounded(&publication.tenant_id, MAX_TENANT_BYTES) {
        return Err(LiveFeedStoreError::InvalidTenant);
    }
    if !is_optional_bounded(publication.repository_id.as_deref(), MAX_REPOSITORY_BYTES) {
        return Err(LiveFeedStoreError::InvalidRepository);
    }
    if !is_bounded(&publication.resource_id, MAX_RESOURCE_ID_BYTES) {
        return Err(LiveFeedStoreError::InvalidResource);
    }
    if !is_bounded(&publication.published_by, MAX_IDENTITY_BYTES) {
        return Err(LiveFeedStoreError::InvalidPublisher);
    }
    if publication.source_digest.len() != SHA256_DIGEST_BYTES {
        return Err(LiveFeedStoreError::InvalidSourceDigest);
    }
    Ok(())
}

pub(super) fn validate_read(
    tenant_id: &str,
    repository_id: Option<&str>,
    cursor: Option<&str>,
    limit: i64,
) -> Result<(), LiveFeedStoreError> {
    if !is_bounded(tenant_id, MAX_TENANT_BYTES) {
        return Err(LiveFeedStoreError::InvalidTenant);
    }
    if !is_optional_bounded(repository_id, MAX_REPOSITORY_BYTES) {
        return Err(LiveFeedStoreError::InvalidRepository);
    }
    if cursor.is_some_and(|cursor| !is_bounded(cursor, MAX_CURSOR_BYTES)) {
        return Err(LiveFeedStoreError::InvalidCursor);
    }
    if !(1..=MAX_REPLAY_LIMIT).contains(&limit) {
        return Err(LiveFeedStoreError::InvalidLimit);
    }
    Ok(())
}

pub(super) fn row_to_event(row: &Row) -> Result<LiveFeedEvent, LiveFeedStoreError> {
    let event_kind: i16 = row.get("event_kind");
    let resource_type: i16 = row.get("resource_type");
    let change_kind: i16 = row.get("change_kind");
    let projection_freshness: Option<i16> = row.get("projection_freshness");
    Ok(LiveFeedEvent {
        cursor: row.get("cursor"),
        tenant_id: row.get("tenant_id"),
        repository_id: row.get("repository_id"),
        event_kind: LiveFeedEventKind::from_i16(event_kind)
            .ok_or(LiveFeedStoreError::UnknownEventKind(event_kind))?,
        resource_type: LiveFeedResourceType::from_i16(resource_type)
            .ok_or(LiveFeedStoreError::UnknownResourceType(resource_type))?,
        resource_id: row.get("resource_id"),
        change_kind: LiveFeedChangeKind::from_i16(change_kind)
            .ok_or(LiveFeedStoreError::UnknownChangeKind(change_kind))?,
        resource_version: row.get("resource_version"),
        source_ledger_position: row.get("source_ledger_position"),
        projection_position: row.get("projection_position"),
        projection_freshness: projection_freshness
            .map(|value| {
                LiveFeedProjectionFreshness::from_i16(value)
                    .ok_or(LiveFeedStoreError::UnknownProjectionFreshness(value))
            })
            .transpose()?,
        snapshot_reload: row.get("snapshot_reload"),
        source_digest: row.get("source_digest"),
        published_by: row.get("published_by"),
        emitted_at: row.get("emitted_at"),
    })
}

pub(super) fn unique_cursor() -> Result<String, LiveFeedStoreError> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| LiveFeedStoreError::Random(error.to_string()))?;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("live-{hex}"))
}

fn is_bounded(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum
}

fn is_optional_bounded(value: Option<&str>, maximum: usize) -> bool {
    value.is_none_or(|value| is_bounded(value, maximum))
}

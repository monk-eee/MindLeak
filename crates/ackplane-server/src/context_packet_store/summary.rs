//! Bounded, payload-free ContextPacket summary reads for Bridge inspection.

use std::time::{SystemTime, UNIX_EPOCH};

use super::{ContextPacketStore, ContextPacketStoreError};

const DEFAULT_SUMMARY_LIMIT: i64 = 20;
const MAX_SUMMARY_LIMIT: i64 = 100;

/// The only packet metadata a Bridge inspection view may expose. The payload
/// and its selected/excluded materials remain internal to the packet domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPacketSummary {
    pub packet_id: String,
    pub task_id: String,
    pub goal_id: String,
    pub agent_session_id: String,
    pub compiler_version: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub ledger_position: u64,
    pub projection_position: u64,
    pub token_budget_requested: u32,
    pub token_budget_used: u32,
    pub lifecycle: ContextPacketLifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextPacketLifecycle {
    Active,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPacketSummaryCursor {
    pub issued_at: i64,
    pub packet_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextPacketSummaryPage {
    pub entries: Vec<ContextPacketSummary>,
    pub effective_limit: i64,
    pub next_cursor: Option<ContextPacketSummaryCursor>,
}

impl ContextPacketStore {
    /// Lists bounded packet metadata newest first. The query deliberately
    /// selects no payload column, so a browser-facing caller cannot obtain
    /// selected context, exclusions, or rendered source material by accident.
    pub async fn list_packet_summaries(
        &self,
        tenant_id: &str,
        repository_id: &str,
        before: Option<&ContextPacketSummaryCursor>,
        requested_limit: Option<u32>,
    ) -> Result<ContextPacketSummaryPage, ContextPacketStoreError> {
        let effective_limit = requested_limit
            .map(i64::from)
            .unwrap_or(DEFAULT_SUMMARY_LIMIT)
            .clamp(1, MAX_SUMMARY_LIMIT);
        if before.is_some_and(|cursor| cursor.packet_id.is_empty() || cursor.packet_id.len() > 256)
        {
            return Err(ContextPacketStoreError::InvalidSummaryCursor);
        }
        let cursor_issued_at = before.map(|cursor| cursor.issued_at);
        let cursor_packet_id = before.map(|cursor| cursor.packet_id.as_str());
        let rows = self
            .client
            .query(
                "SELECT packet_id, task_id, goal_id, agent_session_id, compiler_version, \
                        issued_at, expires_at, ledger_position, projection_position, \
                        token_budget_requested, token_budget_used \
                 FROM context_packets \
                 WHERE tenant_id = $1 AND repository_id = $2 \
                   AND ($3::bigint IS NULL OR (issued_at, packet_id) < ($3::bigint, $4::text)) \
                 ORDER BY issued_at DESC, packet_id DESC \
                 LIMIT $5",
                &[
                    &tenant_id,
                    &repository_id,
                    &cursor_issued_at,
                    &cursor_packet_id,
                    &(effective_limit + 1),
                ],
            )
            .await?;
        let now = unix_seconds(SystemTime::now())?;
        let mut entries = rows
            .into_iter()
            .map(|row| {
                let issued_at: i64 = row.get("issued_at");
                let expires_at: i64 = row.get("expires_at");
                Ok(ContextPacketSummary {
                    packet_id: row.get("packet_id"),
                    task_id: row.get("task_id"),
                    goal_id: row.get("goal_id"),
                    agent_session_id: row.get("agent_session_id"),
                    compiler_version: row.get("compiler_version"),
                    issued_at,
                    expires_at,
                    ledger_position: read_u64(row.get("ledger_position"), "ledger_position")?,
                    projection_position: read_u64(
                        row.get("projection_position"),
                        "projection_position",
                    )?,
                    token_budget_requested: read_u32(
                        row.get("token_budget_requested"),
                        "token_budget_requested",
                    )?,
                    token_budget_used: read_u32(row.get("token_budget_used"), "token_budget_used")?,
                    lifecycle: if expires_at <= now {
                        ContextPacketLifecycle::Expired
                    } else {
                        ContextPacketLifecycle::Active
                    },
                })
            })
            .collect::<Result<Vec<_>, ContextPacketStoreError>>()?;
        let next_cursor = if entries.len() > effective_limit as usize {
            entries.pop();
            entries.last().map(|entry| ContextPacketSummaryCursor {
                issued_at: entry.issued_at,
                packet_id: entry.packet_id.clone(),
            })
        } else {
            None
        };
        Ok(ContextPacketSummaryPage {
            entries,
            effective_limit,
            next_cursor,
        })
    }
}

fn unix_seconds(timestamp: SystemTime) -> Result<i64, ContextPacketStoreError> {
    let elapsed = timestamp.duration_since(UNIX_EPOCH).map_err(|_| {
        ContextPacketStoreError::InvalidStoredNumber {
            field: "server_time",
        }
    })?;
    i64::try_from(elapsed.as_secs()).map_err(|_| ContextPacketStoreError::InvalidStoredNumber {
        field: "server_time",
    })
}

fn read_u32(value: i32, field: &'static str) -> Result<u32, ContextPacketStoreError> {
    u32::try_from(value).map_err(|_| ContextPacketStoreError::InvalidStoredNumber { field })
}

fn read_u64(value: i64, field: &'static str) -> Result<u64, ContextPacketStoreError> {
    u64::try_from(value).map_err(|_| ContextPacketStoreError::InvalidStoredNumber { field })
}

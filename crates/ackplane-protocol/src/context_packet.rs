//! Storage-independent ContextPacket types for the Industrial guidance loop.
//!
//! This module establishes ADR-0114's shared packet and receipt contract. A
//! future ContextService and durable store own transport, authorization, and
//! retention; this protocol model only makes their data and invariants explicit.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// An immutable, bounded snapshot of the context compiled for one agent task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPacket {
    pub packet_id: String,
    pub scope: ContextPacketScope,
    pub compiler_version: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub source: ContextPacketSource,
    pub token_budget: ContextTokenBudget,
    pub selected: Vec<ContextSelection>,
    pub excluded: Vec<ContextExclusion>,
}

impl ContextPacket {
    /// Reject malformed packet data before it reaches a future transport or store.
    pub fn validate(&self) -> Result<(), ContextPacketError> {
        require_non_empty("packet_id", &self.packet_id)?;
        require_non_empty("compiler_version", &self.compiler_version)?;
        self.scope.validate()?;

        if self.expires_at <= self.issued_at {
            return Err(ContextPacketError::ExpiryMustFollowIssuance);
        }
        if self.token_budget.used > self.token_budget.requested {
            return Err(ContextPacketError::TokenBudgetExceeded {
                requested: self.token_budget.requested,
                used: self.token_budget.used,
            });
        }

        let mut selected_ids = HashSet::new();
        for selection in &self.selected {
            selection.validate()?;
            if !selected_ids.insert(selection.item_id.as_str()) {
                return Err(ContextPacketError::DuplicateSelectedItem {
                    item_id: selection.item_id.clone(),
                });
            }
        }

        for exclusion in &self.excluded {
            exclusion.validate()?;
            if selected_ids.contains(exclusion.item_id.as_str()) {
                return Err(ContextPacketError::SelectedAndExcluded {
                    item_id: exclusion.item_id.clone(),
                });
            }
        }

        Ok(())
    }
}

/// The tenant, repository, task, goal, and agent session one packet may serve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPacketScope {
    pub tenant_id: String,
    pub repository_id: String,
    pub task_id: String,
    pub goal_id: String,
    pub agent_session_id: String,
}

impl ContextPacketScope {
    pub fn validate(&self) -> Result<(), ContextPacketError> {
        for (field, value) in [
            ("tenant_id", self.tenant_id.as_str()),
            ("repository_id", self.repository_id.as_str()),
            ("task_id", self.task_id.as_str()),
            ("goal_id", self.goal_id.as_str()),
            ("agent_session_id", self.agent_session_id.as_str()),
        ] {
            require_non_empty(field, value)?;
        }
        Ok(())
    }
}

/// The authoritative positions from which a packet was compiled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPacketSource {
    pub ledger_position: u64,
    pub projection_position: u64,
}

/// Requested and actual token use for one packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextTokenBudget {
    pub requested: u32,
    pub used: u32,
}

/// One source item included in a packet and why it was selected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextSelection {
    pub item_id: String,
    pub item_kind: ContextItemKind,
    pub source_reference: String,
    pub source_version: String,
    pub reason: ContextSelectionReason,
    pub estimated_tokens: u32,
    pub mandatory: bool,
}

impl ContextSelection {
    fn validate(&self) -> Result<(), ContextPacketError> {
        require_non_empty("selected.item_id", &self.item_id)?;
        require_non_empty("selected.source_reference", &self.source_reference)?;
        require_non_empty("selected.source_version", &self.source_version)
    }
}

/// The kind of source a context packet may include.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextItemKind {
    Governance,
    Task,
    Evidence,
    Knowledge,
    Outcome,
    Structural,
}

/// The deterministic reason a source item was selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSelectionReason {
    RequiredGovernance,
    ExplicitTaskReference,
    EvidenceLink,
    GraphReach,
    SemanticSimilarity,
    WorkingSet,
}

/// One candidate deliberately left outside a packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextExclusion {
    pub item_id: String,
    pub reason: ContextExclusionReason,
}

impl ContextExclusion {
    fn validate(&self) -> Result<(), ContextPacketError> {
        require_non_empty("excluded.item_id", &self.item_id)
    }
}

/// Why a candidate was not included in a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextExclusionReason {
    Budget,
    Unauthorized,
    Stale,
    Retired,
    OutOfScope,
    MissingEvidence,
}

/// A supervisor's attributed observation of how it handled one packet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextPacketUseReceipt {
    pub packet_id: String,
    pub scope: ContextPacketScope,
    pub occurred_at: i64,
    pub status: ContextPacketUseStatus,
    pub reason: Option<ContextPacketUseReason>,
}

impl ContextPacketUseReceipt {
    pub fn validate(&self) -> Result<(), ContextPacketError> {
        require_non_empty("packet_id", &self.packet_id)?;
        self.scope.validate()
    }
}

/// The observed lifecycle state of a delivered context packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPacketUseStatus {
    Received,
    Accepted,
    AppliedToPlanning,
    Superseded,
    Refused,
    Expired,
}

/// The typed reason attached to a non-happy-path packet-use receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPacketUseReason {
    Superseded,
    UnsupportedVersion,
    OutOfScope,
    IntegrityMismatch,
    PolicyChanged,
}

/// A deterministic packet-contract validation refusal.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContextPacketError {
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },
    #[error("context packet expiry must follow issuance")]
    ExpiryMustFollowIssuance,
    #[error("context packet used {used} tokens but its budget is {requested}")]
    TokenBudgetExceeded { requested: u32, used: u32 },
    #[error("context packet selected item {item_id:?} more than once")]
    DuplicateSelectedItem { item_id: String },
    #[error("context packet item {item_id:?} is both selected and excluded")]
    SelectedAndExcluded { item_id: String },
}

fn require_non_empty(field: &'static str, value: &str) -> Result<(), ContextPacketError> {
    if value.trim().is_empty() {
        return Err(ContextPacketError::EmptyField { field });
    }
    Ok(())
}

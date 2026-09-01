//! A supervisor's attributed observation of how it handled one delivered
//! packet.
//!
//! Deliberately its own record rather than a field on the packet: a packet is
//! immutable at compilation, so delivery and use progress cannot live on it
//! without reopening what its digest seals.

use serde::{Deserialize, Serialize};

use super::{require_non_empty, ContextPacketError, ContextPacketScope};

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

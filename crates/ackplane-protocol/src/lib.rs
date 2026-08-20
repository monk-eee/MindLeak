//! Versioned wire contracts for repository-node synchronization with Ackplane.
//!
//! These generated types intentionally describe domain records and receipts only.
//! They do not expose local storage schemas or MCP tool payloads.

pub mod v1 {
    tonic::include_proto!("mindleak.ackplane.v1");
}

pub mod claim_auth;
pub mod knowledge_auth;
mod signing_bytes;

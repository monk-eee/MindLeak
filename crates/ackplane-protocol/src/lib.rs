//! Versioned wire contracts for repository-node synchronization with Ackplane.
//!
//! These generated types intentionally describe domain records and receipts only.
//! They do not expose local storage schemas or MCP tool payloads.

pub mod v1 {
    // Every generated service method returns `Result<_, tonic::Status>` --
    // `tonic::Status` itself is the "large" Err variant clippy is measuring,
    // not a choice this generated code makes. Boxing it would mean boxing a
    // type this crate does not define, in code this crate does not write, so
    // this is a lint to silence for generated tonic output, not a signature
    // to redesign method by method.
    #![allow(clippy::result_large_err)]
    tonic::include_proto!("mindleak.ackplane.v1");
}

pub mod claim_auth;
pub mod constitution_auth;
pub mod context_packet;
pub mod delegation;
pub mod knowledge_auth;
mod signing_bytes;
pub mod supervisor;
pub mod telemetry_auth;

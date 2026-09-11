//! Provider implementations of [`crate::NodeSigner`].
//!
//! Explicit software providers: memory-only and OS-credential-backed. Neither
//! claims hardware non-exportability. Hardware/workload providers and runtime
//! enrollment integration remain separate requirements under ADR-0100.

pub mod credential;
pub mod software;

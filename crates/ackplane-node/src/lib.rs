//! `ackplane-node`: the repository-side identity owner (ADR-0100).
//!
//! A full-server repository runs one `ackplane-node` companion that owns the
//! repository's enrolled identity, non-exporting signer, and outbound Ackplane
//! clients. Local planes (`lodestar-mcp`, `mindleak-mcp`) never load, export,
//! or persist the private key themselves — they speak to this crate's narrow
//! `NodeSigner` capability instead.
//!
//! This crate also ships a repository-scoped local IPC endpoint (`ipc`,
//! ADR-0100 decision 4): a Windows named pipe or a Unix-domain socket that
//! accepts only the closed `NodeSigner` operations above, never a TCP
//! listener and never a reusable bearer token. OS-backed providers (Windows
//! CNG, macOS Keychain/Secure Enclave, Linux PKCS#11/TPM), enrolment/restart
//! recovery, and key rotation are separate, narrow follow-on slices.

mod process_lock;
mod provider;
mod signer;

pub mod ipc;

pub use process_lock::{LockError, NodeProcessLock};
pub use provider::software::SoftwareProvider;
pub use signer::{KeyHandle, NodeIdentity, NodeSigner, NodeSignerError, Signature, SigningBinding};

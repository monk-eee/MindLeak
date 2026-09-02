//! `ackplane-node`: the repository-side identity owner (ADR-0100).
//!
//! A full-server repository runs one `ackplane-node` companion that owns the
//! repository's enrolled identity, non-exporting signer, and outbound Ackplane
//! clients. Local planes (`lodestar-mcp`, `mindleak-mcp`) never load, export,
//! or persist the private key themselves — they speak to this crate's narrow
//! `NodeSigner` capability instead.
//!
//! This slice ships the `NodeSigner` contract and a development-only software
//! provider (ADR-0100 decision 5's explicit dev carve-out). OS-backed
//! providers (Windows CNG, macOS Keychain/Secure Enclave, Linux PKCS#11/TPM)
//! are deliberately out of scope here and land as separate, narrow follow-on
//! slices.

mod process_lock;
mod provider;
mod signer;

pub use process_lock::{LockError, NodeProcessLock};
pub use provider::software::SoftwareProvider;
pub use signer::{KeyHandle, NodeIdentity, NodeSigner, NodeSignerError, Signature, SigningBinding};

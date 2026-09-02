//! Provider implementations of [`crate::NodeSigner`].
//!
//! Only the development-only software provider ships in this slice. OS-backed
//! providers (Windows CNG, macOS Keychain/Secure Enclave, Linux PKCS#11/TPM)
//! are separate, narrow follow-on slices — see ADR-0100 decision 5.

pub mod software;

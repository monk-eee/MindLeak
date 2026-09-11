//! `ackplane-node`: the repository-side identity owner (ADR-0100).
//!
//! Building blocks for the companion ADR-0100 assigns ownership of enrolled
//! identity and outbound clients. This library is not yet a runnable companion
//! integrated with the local planes or supervisor.
//!
//! This crate also ships a repository-scoped local IPC endpoint (`ipc`,
//! ADR-0100 decision 4): a Windows named pipe or a Unix-domain socket that
//! accepts only the closed `NodeSigner` operations above, never a TCP
//! listener and never a reusable bearer token; and enrolment + restart
//! identity recovery (`enrolment`, ADR-0100 decision 7). `CredentialProvider`
//! explicitly selects software signing with an OS-credential-backed seed and
//! checked restart identity. It exports no key through `NodeSigner`, but is not
//! a hardware non-exportable key. `CredentialCandidate` persists enrollment
//! challenges and binds accepted authority responses to that same key. Request
//! orchestration, runtime integration, persistent rotation, and hardware/workload
//! providers remain separate requirements. `CredentialProvider::open_connection`
//! authenticates through the reusable client with a private, fallible signer
//! adapter and the provider's recorded binding.

mod enrolment;
mod process_lock;
mod provider;
mod signer;

pub mod ipc;

pub use enrolment::{
    EnrollmentActivation, EnrollmentChallengeRecord, EnrolmentError, EnrolmentRecord,
};
pub use process_lock::{LockError, NodeProcessLock};
pub use provider::credential::{CredentialCandidate, CredentialProvider, CredentialProviderError};
pub use provider::software::SoftwareProvider;
pub use signer::{
    CandidateIdentity, KeyHandle, NodeIdentity, NodeSigner, NodeSignerError, Signature,
    SigningBinding,
};

//! `ackplane-node`: the repository-side identity owner (ADR-0100).
//!
//! `register-me serve` runs this crate's provider-owning companion. Local
//! runtimes use bounded, scoped domain operations over a protected Unix socket
//! or Windows named pipe, never private-key access or a generic signing oracle.
//! Provider loss or refusal of enrolled authority closes every active stream.
//!
//! `CredentialProvider` explicitly selects software custody in the OS credential
//! facility. It is not a hardware non-exportable key. Recovery checks the exact
//! public binding and activation receipt; persistent rotation and hardware
//! providers remain separate requirements.

pub mod companion;
mod enrolment;
mod process_lock;
mod provider;
mod signer;

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

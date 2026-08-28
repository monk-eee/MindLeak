//! Local durable directive inbox for one enrolled supervisor session.
//!
//! The implementation lives in [`inbox`], keeping this crate root focused on
//! the stable public surface that future NodeSync and worker adapters consume.
//!
//! [`daemon`] assembles that surface into the runnable `ackplane-supervisor`
//! binary (ADR-0116: an enrolled supervisor is the only Industrial runtime
//! endpoint).

#![forbid(unsafe_code)]

pub mod config;
pub mod daemon;

mod inbox;
mod outbox;
mod reconcile;
mod storage;
mod worker_adapter;

pub use inbox::{InboxError, SupervisorInbox};
pub use outbox::{OutboxError, OutboxPositions, QueueOutcome, QueuedFrame, SupervisorOutbox};
pub use reconcile::{reconcile, Reconciliation};
pub use worker_adapter::{AdapterError, ProcessWorkerAdapter, WorkerAdapter, WorkerAssignment};

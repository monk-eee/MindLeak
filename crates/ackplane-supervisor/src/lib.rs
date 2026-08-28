//! Local durable directive inbox for one enrolled supervisor session.
//!
//! The implementation lives in [`inbox`], keeping this crate root focused on
//! the stable public surface that future NodeSync and worker adapters consume.

#![forbid(unsafe_code)]

mod inbox;
mod outbox;
mod reconcile;
mod storage;
mod worker_adapter;

pub use inbox::{InboxError, SupervisorInbox};
pub use outbox::{OutboxError, OutboxPositions, QueueOutcome, QueuedFrame, SupervisorOutbox};
pub use reconcile::{reconcile, Reconciliation};
pub use worker_adapter::{AdapterError, ProcessWorkerAdapter, WorkerAdapter, WorkerAssignment};

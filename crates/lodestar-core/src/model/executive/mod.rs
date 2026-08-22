//! Executive types: tasks, their lifecycle events, claims and the scopes and
//! overlaps that coordinate concurrent agents.
//!
//! Split by concern, mirroring the parent `model` module's own pattern:
//! everything below is re-exported, so every `crate::model::executive::X`
//! (and, transitively, `crate::model::X`) path resolves exactly as it did
//! before.
mod board;
mod dialogue;
mod federation;
mod scope;
mod task;

pub use board::*;
pub use dialogue::*;
pub use federation::*;
pub use scope::*;
pub use task::*;

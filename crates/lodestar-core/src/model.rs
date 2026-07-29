//! Domain model for the Lodestar Intent Plane: goals (the constitution), tasks
//! (the executive), conformance verdicts, and consolidated learned knowledge.

// Split by concern. This stays the surface: everything below is re-exported,
// so every `crate::model::X` path resolves exactly as it did before.
mod conformance;
mod constitution;
mod executive;
mod knowledge;

pub use conformance::*;
pub use constitution::*;
pub use executive::*;
pub use knowledge::*;

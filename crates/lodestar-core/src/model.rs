//! Domain model for the Lodestar Intent Plane: goals (the constitution), tasks
//! (the executive), conformance verdicts, and consolidated learned knowledge.
//!
//! Split by those four concerns rather than left as one file. Every type is
//! re-exported here unchanged, so `crate::model::Task` and its siblings resolve
//! exactly as before: a reader gains a smaller file to open, and no call site
//! has to know the split happened.

mod conformance;
mod constitution;
mod executive;
mod knowledge;

pub use conformance::{
    ConformanceCheck, ConformanceEvidence, ConformanceRecord, ConformanceResult,
    EvidenceProvenance, TaskReceipt, Verdict,
};
pub use constitution::{
    Advice, AdviceDisposition, ClauseOrigin, CodeBinding, CodeBindingMode, Consequence,
    ConstitutionProposal, ConstitutionState, ConstitutionStatus, ConstitutionVersion, Goal,
    GoalKind, GoalStatus, GoverningClause,
};
pub use executive::{
    ClaimOverlap, ClaimOverlapReport, ClaimWindow, HumanQuestion, OverlapSignal, Task, TaskEvent,
    TaskEventKind, TaskQa, TaskScope, TaskStatus,
};
pub use knowledge::{Knowledge, SignalPromotion};

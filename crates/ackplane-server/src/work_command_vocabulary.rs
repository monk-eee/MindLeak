//! ADR-0125's closed Work command vocabulary (decision 3: "gRPC, HTTP, and
//! future clients cannot grow different meanings of scope"). This module
//! declares the ten Work command operation names as one canonical, public
//! constant that every consumer -- this crate's own
//! [`crate::work_command_store::model::WorkCommandKind`] (via its
//! `operation_name` method) and the Bridge crate's read-only capability
//! list -- derives from, so no consumer can independently invent or drift
//! from a different vocabulary.
//!
//! Deliberately data-only: it exposes no store, service, or mutation
//! capability. `work_command_store` itself stays crate-internal until
//! authorization and command wiring land (ADR-0125 decision 11).

/// The ten Work commands ADR-0125 permits, in their stable wire order --
/// matching `WorkCommandKind`'s declaration order and persisted `i16`
/// encoding (index 0 is wire value 1, and so on).
pub const WORK_COMMAND_OPERATIONS: [&str; 10] = [
    "create_work",
    "route_work",
    "release_lease",
    "answer_wait",
    "submit_review",
    "assign",
    "steer",
    "pause",
    "resume",
    "drain",
];

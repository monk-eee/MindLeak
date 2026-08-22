//! Argument enums and the deprecated-name table for the design cluster.

use super::Renamed;

pub(super) const DECISIONS: [&str; 8] = [
    "accept",
    "reject",
    "defer",
    "resume",
    "retire",
    "supersede",
    "reopen",
    "attribute",
];
pub(super) const BATCH_DECISIONS: [&str; 4] = ["defer", "resume", "reject", "retire"];
pub(super) const STEPS: [&str; 3] = ["plan", "materialize", "revise"];
pub(super) const VIEWS: [&str; 5] = ["board", "ledger", "promotion", "history", "actions"];

/// The fifteen names this cluster used to answer to, and the call to make now.
///
/// `reconcile_designs` still answers without a session, where `design_register`
/// now requires one, because a deprecation that changes behaviour teaches the
/// wrong lesson.
pub(in crate::tools) const RENAMED: [Renamed; 15] = [
    Renamed {
        old: "register_design",
        new: "design_register",
        key: "",
        value: "",
    },
    Renamed {
        old: "reconcile_designs",
        new: "design_register",
        key: "",
        value: "",
    },
    Renamed {
        old: "accept_design",
        new: "design_decide",
        key: "decision",
        value: "accept",
    },
    Renamed {
        old: "reject_design",
        new: "design_decide",
        key: "decision",
        value: "reject",
    },
    Renamed {
        old: "retire_design",
        new: "design_decide",
        key: "decision",
        value: "retire",
    },
    Renamed {
        old: "supersede_design",
        new: "design_decide",
        key: "decision",
        value: "supersede",
    },
    Renamed {
        old: "reopen_undecided_design",
        new: "design_decide",
        key: "decision",
        value: "reopen",
    },
    Renamed {
        old: "attribute_design_decision",
        new: "design_decide",
        key: "decision",
        value: "attribute",
    },
    Renamed {
        old: "plan_design_promotion",
        new: "design_promote",
        key: "step",
        value: "plan",
    },
    Renamed {
        old: "promote_design",
        new: "design_promote",
        key: "step",
        value: "materialize",
    },
    Renamed {
        old: "revise_design_promotion",
        new: "design_promote",
        key: "step",
        value: "revise",
    },
    Renamed {
        old: "design_board",
        new: "design_query",
        key: "view",
        value: "board",
    },
    Renamed {
        old: "list_designs",
        new: "design_query",
        key: "view",
        value: "ledger",
    },
    Renamed {
        old: "design_promotion",
        new: "design_query",
        key: "view",
        value: "promotion",
    },
    Renamed {
        old: "design_materialization_history",
        new: "design_query",
        key: "view",
        value: "history",
    },
];

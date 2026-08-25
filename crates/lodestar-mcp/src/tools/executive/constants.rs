//! Constants shared by executive tool dispatch: enum-like argument tables and
//! the deprecated-name translation table.

use super::Renamed;

pub(super) const CLAIM_STEPS: [&str; 4] = ["claim", "renew", "release", "recover"];

/// Task summaries `existing_work` and `task_create` will carry, most recent
/// first.
///
/// `existing_work` is every task ever created under a goal or matching a
/// scope, unbounded and un-truncated: creating this very task under a
/// 149-task goal returned 25 KB from this array alone. The exact count still
/// answers "how much", so nothing about the full total is lost — only the
/// list a caller reads is capped, at the size most likely to matter first.
pub(super) const TASK_PREVIEW_LIMIT: usize = 20;

/// `board`'s default cap, newest-first, when the caller does not pass an
/// explicit `limit`.
///
/// `board` is not a preview like `existing_work` — several callers
/// (`evaluate-pr-effectiveness.mjs`, `stranded-report.mjs`) read the whole
/// history on purpose — so it needs headroom past `TASK_PREVIEW_LIMIT`
/// rather than the same tiny number, while still bounding the common case.
/// Measured on this repository's own board at 1,019 tasks (`detail=false`):
/// ~396 bytes/row, ~403 KB unbounded. `limit=0` opts out of the cap entirely
/// for a caller that has already decided it needs the complete history.
pub(super) const BOARD_PREVIEW_LIMIT: usize = 200;

pub(super) const TRANSITIONS: [&str; 9] = [
    "complete", "resolve", "block", "reopen", "abandon", "pause", "resume", "ask", "answer",
];

pub(super) const VIEWS: [&str; 13] = [
    "board",
    "doctor",
    "rework",
    "next",
    "scope",
    "existing_work",
    "overlap",
    "stalled",
    "thread",
    "pending_questions",
    "questions_for_a_human",
    "drafts",
    "claim_transfers",
];

/// The twenty-six names this cluster used to answer to, and the call to make now
/// (ADR-0059).
///
/// `decompose_goal` maps onto `task_create` by shape rather than by argument:
/// it was only ever called with a goal id, and a `task_create` without a title
/// is a decomposition. Everything else names its transition explicitly.
pub(in crate::tools) const RENAMED: [Renamed; 26] = [
    Renamed {
        old: "create_task",
        new: "task_create",
        key: "",
        value: "",
    },
    Renamed {
        old: "decompose_goal",
        new: "task_create",
        key: "",
        value: "",
    },
    Renamed {
        old: "claim_task",
        new: "task_claim",
        key: "step",
        value: "claim",
    },
    Renamed {
        old: "renew_lease",
        new: "task_claim",
        key: "step",
        value: "renew",
    },
    Renamed {
        old: "release_task",
        new: "task_claim",
        key: "step",
        value: "release",
    },
    Renamed {
        old: "recover_claim",
        new: "task_claim",
        key: "step",
        value: "recover",
    },
    Renamed {
        old: "complete_task",
        new: "task_transition",
        key: "to",
        value: "complete",
    },
    Renamed {
        old: "resolve_task",
        new: "task_transition",
        key: "to",
        value: "resolve",
    },
    Renamed {
        old: "block_task",
        new: "task_transition",
        key: "to",
        value: "block",
    },
    Renamed {
        old: "reopen_task",
        new: "task_transition",
        key: "to",
        value: "reopen",
    },
    Renamed {
        old: "abandon_task",
        new: "task_transition",
        key: "to",
        value: "abandon",
    },
    Renamed {
        old: "pause_task",
        new: "task_transition",
        key: "to",
        value: "pause",
    },
    Renamed {
        old: "resume_task",
        new: "task_transition",
        key: "to",
        value: "resume",
    },
    Renamed {
        old: "ask_question",
        new: "task_transition",
        key: "to",
        value: "ask",
    },
    Renamed {
        old: "answer",
        new: "task_transition",
        key: "to",
        value: "answer",
    },
    Renamed {
        old: "board",
        new: "task_query",
        key: "view",
        value: "board",
    },
    Renamed {
        old: "next_task",
        new: "task_query",
        key: "view",
        value: "next",
    },
    Renamed {
        old: "task_scope",
        new: "task_query",
        key: "view",
        value: "scope",
    },
    Renamed {
        old: "existing_work",
        new: "task_query",
        key: "view",
        value: "existing_work",
    },
    Renamed {
        old: "check_overlap",
        new: "task_query",
        key: "view",
        value: "overlap",
    },
    Renamed {
        old: "stalled_work",
        new: "task_query",
        key: "view",
        value: "stalled",
    },
    Renamed {
        old: "task_qa",
        new: "task_query",
        key: "view",
        value: "thread",
    },
    Renamed {
        old: "pending_questions",
        new: "task_query",
        key: "view",
        value: "pending_questions",
    },
    Renamed {
        old: "questions_for_a_human",
        new: "task_query",
        key: "view",
        value: "questions_for_a_human",
    },
    Renamed {
        old: "draft_questions",
        new: "task_query",
        key: "view",
        value: "drafts",
    },
    Renamed {
        old: "claim_transfer_history",
        new: "task_query",
        key: "view",
        value: "claim_transfers",
    },
];

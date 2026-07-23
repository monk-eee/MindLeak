//! Design items (ADR-0023): an ADR under human review.
//!
//! A design item references an ADR file and carries that ADR's lifecycle: the
//! *taint* is the ADR's `Proposed` status. While proposed it is **not**
//! claimable and never appears in `next_task` or the executive board — it lives
//! on the Design Board. A human `accept`/`reject` is the completion path for
//! design work; unlike an implementation task it does **not** run ADR-0009 code
//! conformance (there is no code for a design decision to conform to). Rejection
//! is durable and auditable — archived, never deleted (ADR-0019).

use serde::{Deserialize, Serialize};

use crate::model::{Goal, Task};

/// Where a design item sits in its human-review lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DesignStatus {
    /// Tainted: awaiting a human decision. On the Design Board, off the
    /// executive board.
    Proposed,
    /// A human accepted the design; it is the completion of design work.
    Accepted,
    /// A human rejected the design; durable and auditable, spawns no work.
    Rejected,
}

impl DesignStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            DesignStatus::Proposed => "proposed",
            DesignStatus::Accepted => "accepted",
            DesignStatus::Rejected => "rejected",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "proposed" => Some(DesignStatus::Proposed),
            "accepted" => Some(DesignStatus::Accepted),
            "rejected" => Some(DesignStatus::Rejected),
            _ => None,
        }
    }

    /// A proposed item is the only state a human decision may act on.
    pub fn is_open(&self) -> bool {
        matches!(self, DesignStatus::Proposed)
    }
}

/// An ADR under review in the Intent Plane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignItem {
    /// Stable id derived from the ADR file, e.g. `design:0023-design-board`.
    pub id: String,
    /// The ADR path, normalised to forward slashes, e.g. `docs/adr/0023-...md`.
    pub adr_path: String,
    pub title: String,
    pub summary: String,
    pub status: DesignStatus,
    /// Agent that registered the item (may not decide its own design).
    pub proposed_by: Option<String>,
    /// Human that accepted or rejected it.
    pub decided_by: Option<String>,
    /// Acceptance/rejection rationale.
    pub reason: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// The objective goal spawned when this item was accepted (the ADR-0023
    /// accept→decompose bridge); `None` until accepted.
    pub spawned_goal_id: Option<String>,
}

/// The outcome of accepting a design item (ADR-0023): the accepted item plus the
/// objective goal and claimable implementation tasks the bridge spawned from it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesignAcceptance {
    pub item: DesignItem,
    pub goal: Goal,
    pub tasks: Vec<Task>,
}

/// Derive a stable design-item id from an ADR path: the file stem, prefixed
/// `design:`. `docs/adr/0023-design-board-accept-bridge.md` →
/// `design:0023-design-board-accept-bridge`. Platform-agnostic (handles both
/// slash conventions).
pub fn design_id_from_path(adr_path: &str) -> String {
    let normalized = adr_path.replace('\\', "/");
    let file = normalized.rsplit('/').next().unwrap_or(normalized.as_str());
    let stem = file.strip_suffix(".md").unwrap_or(file);
    format!("design:{stem}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn design_id_is_the_adr_stem_regardless_of_path_convention() {
        assert_eq!(
            design_id_from_path("docs/adr/0023-design-board-accept-bridge.md"),
            "design:0023-design-board-accept-bridge"
        );
        // Backslashes (Windows) normalise identically.
        assert_eq!(
            design_id_from_path("docs\\adr\\0007-structural.md"),
            "design:0007-structural"
        );
        // A bare filename with no directory still works.
        assert_eq!(design_id_from_path("0042-thing.md"), "design:0042-thing");
    }

    #[test]
    fn only_a_proposed_item_is_open_to_a_decision() {
        assert!(DesignStatus::Proposed.is_open());
        assert!(!DesignStatus::Accepted.is_open());
        assert!(!DesignStatus::Rejected.is_open());
    }

    #[test]
    fn status_tags_round_trip() {
        for s in [
            DesignStatus::Proposed,
            DesignStatus::Accepted,
            DesignStatus::Rejected,
        ] {
            assert_eq!(DesignStatus::from_tag(s.as_str()), Some(s));
        }
        assert_eq!(DesignStatus::from_tag("bogus"), None);
    }
}

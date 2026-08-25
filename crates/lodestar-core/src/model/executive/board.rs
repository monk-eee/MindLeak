//! Board diagnostics: conditions worth a person's attention, and the
//! measurable rework rate they help explain.

use serde::{Deserialize, Serialize};

/// A board condition worth a person's attention, and what kind it is.
///
/// Each variant is a shape that was found and repaired by hand before this
/// existed, and none of them is surfaced by any other view: `stalled` reports
/// lateness, and nothing about a duplicate or an ungated block is late.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BoardAilment {
    /// Live tasks under one goal carrying the same title. Twenty-one of these
    /// reached the board from a generator that was additive across runs.
    DuplicateTitle,
    /// Live tasks carrying the same title under different goals. Twenty-eight
    /// arrived in one pass from a generator run once per active goal; only one
    /// of them can be the work, and the rest are graded against goals they do
    /// not serve. Declared breadth belongs on a single task (ADR-0041).
    SameTitleAcrossGoals,
    /// Blocked with no predecessor, so nothing will ever unblock it. Nine of
    /// these accumulated, invisible to every view: `next` skips them, `stalled`
    /// reports lateness and they are not late, and without a reason they name
    /// nothing that would clear them.
    BlockedWithoutGate,
}

impl BoardAilment {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DuplicateTitle => "duplicate_title",
            Self::SameTitleAcrossGoals => "same_title_across_goals",
            Self::BlockedWithoutGate => "blocked_without_gate",
        }
    }
}

/// One diagnosed condition: what it is, which tasks are in it, and what a
/// person could do about it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardFinding {
    pub ailment: BoardAilment,
    /// The tasks involved, oldest first, so the one to keep reads first.
    pub task_ids: Vec<String>,
    /// What the finding is about — a title, or the blocked task's own title.
    pub subject: String,
    /// The suggested repair. A suggestion: this view judges nothing and
    /// changes nothing, because which duplicate is the real work is a call
    /// only the reader can make (ADR-0015).
    pub remedy: String,
}

/// One title that was seeded more than once, and how badly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepeatedTitle {
    pub title: String,
    /// Every task carrying this title, in any state.
    pub seeds: usize,
    /// How many of them were redundant — all but the earliest.
    pub redundant: usize,
    /// Distinct goals the title was seeded under. More than one means the work
    /// was graded against goals it cannot serve.
    pub goals: usize,
}

/// How much of the work the fleet created had already been created.
///
/// [ADR-0057](../../../docs/adr/0057-work-already-done-is-a-collision.md) named
/// the rework rate as the measurable outcome of the whole coordination line,
/// recorded a baseline, and said that if it does not fall the mechanism is
/// wrong and should be removed rather than tuned indefinitely. Nothing could
/// re-run that test, so it never was. This is the instrument.
///
/// A task counts as redundant when an *earlier* task carries its exact title:
/// by the time it existed there was nothing new for it to do. That is
/// deliberately the narrow, provable subset of waste. Abandonment is reported
/// beside it but is NOT called rework — work dropped because it turned out to
/// be unnecessary is good judgement, and counting it as waste would flatter a
/// fleet that never reconsiders anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReworkReport {
    /// Tasks created in the window, in any state.
    pub created: usize,
    pub redundant: usize,
    /// Redundant seeds created in the same second as the task they repeat.
    ///
    /// The signature of a generator, not of an agent: a person or an agent
    /// deciding whether to start cannot produce two tasks in one second. This
    /// is the number that says whether an advisory notice could have helped,
    /// because a notice is addressed to a reader and a generator has none.
    pub same_second: usize,
    pub abandoned: usize,
    /// Worst first, so the reader sees the shape before the total.
    pub repeated_titles: Vec<RepeatedTitle>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn board_ailment_as_str_matches_the_serialized_snake_case_tag() {
        assert_eq!(BoardAilment::DuplicateTitle.as_str(), "duplicate_title");
        assert_eq!(
            BoardAilment::SameTitleAcrossGoals.as_str(),
            "same_title_across_goals"
        );
        assert_eq!(
            BoardAilment::BlockedWithoutGate.as_str(),
            "blocked_without_gate"
        );
    }
}

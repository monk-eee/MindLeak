//! Why work on the board is not moving.
//!
//! Three tasks once sat unfinished for three different reasons and nothing
//! reported any of them: a lease lapsed after the work had shipped, a change
//! landed outside its claim window, and a legitimate cross-plane edit resolved
//! as drift. All were found by accident, and blocked work had queued behind
//! them for days.
//!
//! This is a **read-only report and nothing more**. It states what is true and
//! how long it has been true; it never decides that something has waited *too*
//! long, because that threshold is a judgement about the work, not a property
//! of it. A number invented here would quietly become policy nobody agreed to.
//! For the same reason it changes no task state and produces no verdict — the
//! evidence contract is not involved.

use serde::{Deserialize, Serialize};

use crate::model::{Task, TaskStatus};

/// The shape of a stall. Each variant names something an agent cannot resolve
/// by working harder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StallKind {
    /// Claimed, but the lease expired. Nobody is renewing and no verdict was
    /// ever recorded, so the work may exist in Git and not in the ledger.
    LapsedLease,
    /// Completed and awaiting a human decision. Nothing an agent does moves it.
    AwaitingHuman,
    /// Blocked by a task no agent will advance — one already terminal, or
    /// itself parked or awaiting a human.
    BlockedByUnfinished,
    /// Blocked by a task id that is not on the board at all.
    BlockedByMissingTask,
    /// Parked awaiting an answer, with no answer yet.
    Parked,
}

impl StallKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            StallKind::LapsedLease => "lapsed_lease",
            StallKind::AwaitingHuman => "awaiting_human",
            StallKind::BlockedByUnfinished => "blocked_by_unfinished",
            StallKind::BlockedByMissingTask => "blocked_by_missing_task",
            StallKind::Parked => "parked",
        }
    }
}

/// One stalled task, with the fact that stalled it and how long ago.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stall {
    pub task_id: String,
    pub title: String,
    pub kind: StallKind,
    /// When the stalling fact became true, as precisely as the board records it.
    pub since: i64,
    /// How long it has been true. Reported, never judged.
    pub stalled_seconds: i64,
    /// The specific reason, naming the owner or the blocking task and its state.
    pub detail: String,
}

/// Whether a task is one an agent could still move on its own.
fn agent_can_advance(status: TaskStatus) -> bool {
    match status {
        TaskStatus::Open | TaskStatus::Claimed => true,
        TaskStatus::InReview
        | TaskStatus::NeedsInput
        | TaskStatus::Paused
        | TaskStatus::Done
        | TaskStatus::Blocked
        | TaskStatus::Abandoned => false,
    }
}

/// Report every task that is not progressing, newest stall last.
///
/// Pure over the board so it is testable without a database, and so the rules
/// are readable in one place rather than spread across SQL.
pub fn stalls(tasks: &[Task], now: i64) -> Vec<Stall> {
    let mut found = Vec::new();
    for task in tasks {
        let stall = match task.status {
            TaskStatus::Claimed => match task.lease_expires_at {
                // An expired lease is objective: no threshold is invented here.
                Some(expires) if expires < now => Some((
                    StallKind::LapsedLease,
                    expires,
                    format!(
                        "lease expired; owner {}",
                        task.owner.as_deref().unwrap_or("unknown")
                    ),
                )),
                _ => None,
            },
            TaskStatus::InReview => Some((
                StallKind::AwaitingHuman,
                task.updated_at,
                "completed with a verdict that needs a human decision".to_string(),
            )),
            TaskStatus::NeedsInput | TaskStatus::Paused => Some((
                StallKind::Parked,
                task.parked_at.unwrap_or(task.updated_at),
                "parked awaiting an answer".to_string(),
            )),
            TaskStatus::Blocked => match task.blocked_by.as_deref() {
                Some(blocker_id) => match tasks.iter().find(|other| other.id == blocker_id) {
                    Some(blocker) if !agent_can_advance(blocker.status) => Some((
                        StallKind::BlockedByUnfinished,
                        task.updated_at,
                        format!(
                            "blocked by {blocker_id}, which is '{}' — no agent will advance it",
                            blocker.status.as_str()
                        ),
                    )),
                    // Blocked behind live work is waiting, not stalling.
                    Some(_) => None,
                    None => Some((
                        StallKind::BlockedByMissingTask,
                        task.updated_at,
                        format!("blocked by {blocker_id}, which is not on the board"),
                    )),
                },
                None => None,
            },
            TaskStatus::Open | TaskStatus::Done | TaskStatus::Abandoned => None,
        };
        if let Some((kind, since, detail)) = stall {
            found.push(Stall {
                task_id: task.id.clone(),
                title: task.title.clone(),
                kind,
                since,
                stalled_seconds: (now - since).max(0),
                detail,
            });
        }
    }
    found.sort_by(|left, right| {
        right
            .stalled_seconds
            .cmp(&left.stalled_seconds)
            .then_with(|| left.task_id.cmp(&right.task_id))
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;

    fn task(id: &str, status: TaskStatus) -> Task {
        Task {
            id: id.to_string(),
            goal_id: "goal:x".to_string(),
            parent_task_id: None,
            title: format!("task {id}"),
            acceptance: "done".to_string(),
            status,
            owner: None,
            claim_started_at: None,
            lease_expires_at: None,
            blocked_by: None,
            parked_at: None,
            created_at: NOW - 10_000,
            updated_at: NOW - 10_000,
        }
    }

    fn kinds(found: &[Stall]) -> Vec<(&str, StallKind)> {
        found.iter().map(|s| (s.task_id.as_str(), s.kind)).collect()
    }

    /// The real stall this was built for: work was built and opened as a pull
    /// request, but conformance was never run and the lease quietly expired.
    /// Nothing reported it, so the work existed in Git and not in the ledger.
    #[test]
    fn reports_a_claimed_task_whose_lease_has_expired() {
        let mut claimed = task("task:lapsed", TaskStatus::Claimed);
        claimed.owner = Some("session:v1:copilot:abc".to_string());
        claimed.lease_expires_at = Some(NOW - 3_600);

        let found = stalls(&[claimed], NOW);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, StallKind::LapsedLease);
        assert_eq!(found[0].stalled_seconds, 3_600);
        assert!(found[0].detail.contains("session:v1:copilot:abc"));
    }

    /// A live lease is work in progress, not a stall. Reporting it would train
    /// the reader to ignore the report.
    #[test]
    fn stays_silent_about_a_claim_whose_lease_is_still_live() {
        let mut claimed = task("task:live", TaskStatus::Claimed);
        claimed.lease_expires_at = Some(NOW + 600);
        assert!(stalls(&[claimed], NOW).is_empty());
    }

    #[test]
    fn reports_work_waiting_on_a_human_decision() {
        let mut review = task("task:review", TaskStatus::InReview);
        review.updated_at = NOW - 7_200;

        let found = stalls(&[review], NOW);

        assert_eq!(
            kinds(&found),
            vec![("task:review", StallKind::AwaitingHuman)]
        );
        assert_eq!(found[0].stalled_seconds, 7_200);
    }

    /// The 78-hour case: blocked behind a task that is itself awaiting a human,
    /// so no agent will ever advance it and nothing says so.
    #[test]
    fn reports_a_block_behind_a_task_no_agent_will_advance() {
        let blocker = task("task:blocker", TaskStatus::InReview);
        let mut blocked = task("task:blocked", TaskStatus::Blocked);
        blocked.blocked_by = Some("task:blocker".to_string());

        let found = stalls(&[blocker, blocked], NOW);

        let blocked_row = found
            .iter()
            .find(|s| s.task_id == "task:blocked")
            .expect("the blocked task is reported");
        assert_eq!(blocked_row.kind, StallKind::BlockedByUnfinished);
        assert!(blocked_row.detail.contains("task:blocker"));
        assert!(blocked_row.detail.contains("in_review"));
    }

    /// A block behind a finished task can never clear on its own either.
    #[test]
    fn reports_a_block_behind_an_already_terminal_task() {
        let blocker = task("task:done", TaskStatus::Done);
        let mut blocked = task("task:stale", TaskStatus::Blocked);
        blocked.blocked_by = Some("task:done".to_string());

        let found = stalls(&[blocker, blocked], NOW);

        assert_eq!(
            kinds(&found),
            vec![("task:stale", StallKind::BlockedByUnfinished)]
        );
    }

    /// Waiting behind live work is ordinary sequencing, not a stall.
    #[test]
    fn stays_silent_about_a_block_behind_live_work() {
        let blocker = task("task:open", TaskStatus::Open);
        let mut blocked = task("task:waiting", TaskStatus::Blocked);
        blocked.blocked_by = Some("task:open".to_string());
        assert!(stalls(&[blocker, blocked], NOW).is_empty());
    }

    #[test]
    fn reports_a_block_naming_a_task_that_is_not_on_the_board() {
        let mut blocked = task("task:orphan", TaskStatus::Blocked);
        blocked.blocked_by = Some("task:vanished".to_string());

        let found = stalls(&[blocked], NOW);

        assert_eq!(
            kinds(&found),
            vec![("task:orphan", StallKind::BlockedByMissingTask)]
        );
        assert!(found[0].detail.contains("task:vanished"));
    }

    #[test]
    fn reports_parked_work_from_when_it_was_parked() {
        let mut parked = task("task:parked", TaskStatus::NeedsInput);
        parked.parked_at = Some(NOW - 250_000);

        let found = stalls(&[parked], NOW);

        assert_eq!(kinds(&found), vec![("task:parked", StallKind::Parked)]);
        assert_eq!(found[0].stalled_seconds, 250_000);
    }

    /// Longest-stalled first: the thing that has been ignored most is the thing
    /// most worth seeing.
    #[test]
    fn orders_the_longest_stall_first_and_ignores_finished_work() {
        let mut recent = task("task:recent", TaskStatus::InReview);
        recent.updated_at = NOW - 60;
        let mut old = task("task:old", TaskStatus::InReview);
        old.updated_at = NOW - 90_000;

        let found = stalls(
            &[
                recent,
                old,
                task("task:done", TaskStatus::Done),
                task("task:gone", TaskStatus::Abandoned),
                task("task:open", TaskStatus::Open),
            ],
            NOW,
        );

        assert_eq!(
            found.iter().map(|s| s.task_id.as_str()).collect::<Vec<_>>(),
            vec!["task:old", "task:recent"]
        );
    }
}

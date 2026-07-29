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

use crate::fleet::{wait_cycles, Wait};
use crate::model::{Task, TaskStatus};

/// The shape of a stall. Each variant names something an agent cannot resolve
/// by working harder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StallKind {
    /// Claimed, but the lease expired. Nobody is renewing and no verdict was
    /// ever recorded, so the work may exist in Git and not in the ledger.
    LapsedLease,
    /// Waiting on a person: completed and awaiting a human decision, or parked
    /// on a question addressed to nobody in particular. Nothing an agent does
    /// moves it.
    AwaitingHuman,
    /// Parked on a question addressed to a specific peer agent (ADR-0046). That
    /// peer can still answer, so this is a live wait rather than a dead one —
    /// but it names who has to act, which `awaiting_human` would not.
    AwaitingAgent,
    /// Parked on a peer who is themselves parked on this agent, directly or
    /// through a ring (ADR-0046). Nobody in the set can move, so waiting is not
    /// a strategy: it needs an answer from outside, or the parking grace.
    Deadlocked,
    /// Blocked by a task no agent will advance — one already terminal, or
    /// itself parked or awaiting a human.
    BlockedByUnfinished,
    /// Blocked by a task id that is not on the board at all.
    BlockedByMissingTask,
    /// Deliberately suspended by its owner. Nobody was asked anything, so
    /// nobody owes an answer; only the owner resumes it.
    Paused,
}

impl StallKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            StallKind::LapsedLease => "lapsed_lease",
            StallKind::AwaitingHuman => "awaiting_human",
            StallKind::AwaitingAgent => "awaiting_agent",
            StallKind::Deadlocked => "deadlocked",
            StallKind::BlockedByUnfinished => "blocked_by_unfinished",
            StallKind::BlockedByMissingTask => "blocked_by_missing_task",
            StallKind::Paused => "paused",
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
/// Pure over the board and the wait graph so it is testable without a database,
/// and so the rules are readable in one place rather than spread across SQL.
///
/// `waits` is the same unanswered-addressed-question set the fleet view reads
/// (ADR-0046). It is passed in rather than re-derived so the two surfaces
/// cannot disagree about who is waiting on whom: this one answers per task,
/// the fleet view answers per agent, and both are the same fact.
pub fn stalls(tasks: &[Task], waits: &[Wait], now: i64) -> Vec<Stall> {
    let deadlocked: Vec<String> = wait_cycles(waits)
        .into_iter()
        .flat_map(|cycle| cycle.task_ids)
        .collect();
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
            TaskStatus::NeedsInput => {
                let since = task.parked_at.unwrap_or(task.updated_at);
                Some(match waits.iter().find(|wait| wait.task_id == task.id) {
                    Some(wait) if deadlocked.contains(&task.id) => (
                        StallKind::Deadlocked,
                        since,
                        format!(
                            "parked on {}, who is waiting on this agent in turn; \
                             only an answer from outside breaks it",
                            wait.waited_on
                        ),
                    ),
                    Some(wait) => (
                        StallKind::AwaitingAgent,
                        since,
                        format!("parked awaiting an answer from {}", wait.waited_on),
                    ),
                    None => (
                        StallKind::AwaitingHuman,
                        since,
                        "parked awaiting an answer from a human".to_string(),
                    ),
                })
            }
            TaskStatus::Paused => Some((
                StallKind::Paused,
                task.parked_at.unwrap_or(task.updated_at),
                format!(
                    "deliberately suspended by {}",
                    task.owner.as_deref().unwrap_or("its owner")
                ),
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
            resolved_by: None,
            resolved_at: None,
            resolved_conformance_id: None,
            created_at: NOW - 10_000,
            updated_at: NOW - 10_000,
        }
    }

    fn kinds(found: &[Stall]) -> Vec<(&str, StallKind)> {
        found.iter().map(|s| (s.task_id.as_str(), s.kind)).collect()
    }

    /// Most cases do not involve anyone waiting on anyone.
    fn no_waits() -> Vec<Wait> {
        Vec::new()
    }

    fn waiting(task_id: &str, waiter: &str, waited_on: &str) -> Wait {
        Wait {
            task_id: task_id.to_string(),
            waiter: waiter.to_string(),
            waited_on: waited_on.to_string(),
            asked_at: NOW - 1_000,
        }
    }

    /// The real stall this was built for: work was built and opened as a pull
    /// request, but conformance was never run and the lease quietly expired.
    /// Nothing reported it, so the work existed in Git and not in the ledger.
    #[test]
    fn reports_a_claimed_task_whose_lease_has_expired() {
        let mut claimed = task("task:lapsed", TaskStatus::Claimed);
        claimed.owner = Some("session:v1:copilot:abc".to_string());
        claimed.lease_expires_at = Some(NOW - 3_600);

        let found = stalls(&[claimed], &no_waits(), NOW);

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
        assert!(stalls(&[claimed], &no_waits(), NOW).is_empty());
    }

    #[test]
    fn reports_work_waiting_on_a_human_decision() {
        let mut review = task("task:review", TaskStatus::InReview);
        review.updated_at = NOW - 7_200;

        let found = stalls(&[review], &no_waits(), NOW);

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

        let found = stalls(&[blocker, blocked], &no_waits(), NOW);

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

        let found = stalls(&[blocker, blocked], &no_waits(), NOW);

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
        assert!(stalls(&[blocker, blocked], &no_waits(), NOW).is_empty());
    }

    #[test]
    fn reports_a_block_naming_a_task_that_is_not_on_the_board() {
        let mut blocked = task("task:orphan", TaskStatus::Blocked);
        blocked.blocked_by = Some("task:vanished".to_string());

        let found = stalls(&[blocked], &no_waits(), NOW);

        assert_eq!(
            kinds(&found),
            vec![("task:orphan", StallKind::BlockedByMissingTask)]
        );
        assert!(found[0].detail.contains("task:vanished"));
    }

    /// ADR-0046: a park with no addressed question is a park on a person. The
    /// pre-fold report called every park "parked awaiting an answer", which said
    /// nothing about who owed it.
    #[test]
    fn reports_parked_work_from_when_it_was_parked() {
        let mut parked = task("task:parked", TaskStatus::NeedsInput);
        parked.parked_at = Some(NOW - 250_000);

        let found = stalls(&[parked], &no_waits(), NOW);

        assert_eq!(
            kinds(&found),
            vec![("task:parked", StallKind::AwaitingHuman)]
        );
        assert_eq!(found[0].stalled_seconds, 250_000);
        assert!(found[0].detail.contains("human"));
    }

    /// ADR-0046: a park on a named peer is a live wait, and naming the peer is
    /// the whole point — "awaiting a human" would send the reader to the wrong
    /// person, which is worse than saying nothing.
    #[test]
    fn a_park_on_a_peer_names_the_peer_rather_than_a_human() {
        let mut parked = task("task:asked", TaskStatus::NeedsInput);
        parked.parked_at = Some(NOW - 400);

        let found = stalls(
            &[parked],
            &[waiting("task:asked", "agent-a", "agent-b")],
            NOW,
        );

        assert_eq!(
            kinds(&found),
            vec![("task:asked", StallKind::AwaitingAgent)]
        );
        assert!(found[0].detail.contains("agent-b"));
    }

    /// ADR-0046: the case both halves of this fold exist for. Two agents parked
    /// on each other are not waiting, they are stuck, and before the fold this
    /// report rendered them as two ordinary parked rows.
    #[test]
    fn a_mutual_park_is_reported_as_a_deadlock_not_an_ordinary_wait() {
        let mut first = task("task:one", TaskStatus::NeedsInput);
        first.parked_at = Some(NOW - 900);
        let mut second = task("task:two", TaskStatus::NeedsInput);
        second.parked_at = Some(NOW - 900);

        let found = stalls(
            &[first, second],
            &[
                waiting("task:one", "agent-a", "agent-b"),
                waiting("task:two", "agent-b", "agent-a"),
            ],
            NOW,
        );

        assert_eq!(
            kinds(&found),
            vec![
                ("task:one", StallKind::Deadlocked),
                ("task:two", StallKind::Deadlocked)
            ]
        );
        assert!(found[0].detail.contains("outside"));
    }

    /// A deliberate pause is not an unanswered question. Nobody was asked, so
    /// reporting it as awaiting an answer would send someone looking for a
    /// person who owes nothing.
    #[test]
    fn a_deliberate_pause_is_not_reported_as_awaiting_an_answer() {
        let mut paused = task("task:paused", TaskStatus::Paused);
        paused.owner = Some("agent-a".to_string());
        paused.parked_at = Some(NOW - 60);

        let found = stalls(&[paused], &no_waits(), NOW);

        assert_eq!(kinds(&found), vec![("task:paused", StallKind::Paused)]);
        assert!(found[0].detail.contains("agent-a"));
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
            &no_waits(),
            NOW,
        );

        assert_eq!(
            found.iter().map(|s| s.task_id.as_str()).collect::<Vec<_>>(),
            vec!["task:old", "task:recent"]
        );
    }
}

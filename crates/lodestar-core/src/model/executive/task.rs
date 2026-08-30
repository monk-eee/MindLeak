//! Task lifecycle: status, events, the evidence-window claim, and the row
//! itself.

use serde::{Deserialize, Serialize};

/// Lifecycle of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Open,
    Claimed,
    /// Owner parked the task with a durable question awaiting a human answer
    /// (ADR-0020): live lease cleared, owner + evidence window retained.
    NeedsInput,
    /// Owner deliberately suspended the task (ADR-0020): live lease cleared,
    /// owner + evidence window retained, resumable by the same owner.
    Paused,
    InReview,
    Done,
    Blocked,
    Abandoned,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Open => "open",
            TaskStatus::Claimed => "claimed",
            TaskStatus::NeedsInput => "needs_input",
            TaskStatus::Paused => "paused",
            TaskStatus::InReview => "in_review",
            TaskStatus::Done => "done",
            TaskStatus::Blocked => "blocked",
            TaskStatus::Abandoned => "abandoned",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "open" => Some(TaskStatus::Open),
            "claimed" => Some(TaskStatus::Claimed),
            "needs_input" => Some(TaskStatus::NeedsInput),
            "paused" => Some(TaskStatus::Paused),
            "in_review" => Some(TaskStatus::InReview),
            "done" => Some(TaskStatus::Done),
            "blocked" => Some(TaskStatus::Blocked),
            "abandoned" => Some(TaskStatus::Abandoned),
            _ => None,
        }
    }
}

/// What happened to a task (ADR-0064).
///
/// One variant per transition the executive can perform. These are the verbs of
/// the task lifecycle; `TaskStatus` is the noun they leave behind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskEventKind {
    /// Genesis for a task that existed before the log did. It carries the state
    /// at migration time and **no history before it** — see ADR-0064. The
    /// absence of earlier events for such a task is a fact about this database,
    /// not a gap to be filled in with plausible reconstruction.
    Imported,
    Created,
    Claimed,
    LeaseRenewed,
    Released,
    Blocked,
    Reopened,
    Abandoned,
    /// Parked with a durable question (ADR-0020).
    Questioned,
    Answered,
    Paused,
    Resumed,
    /// Ownership moved by audited recovery rather than by claim (ADR-0030).
    ClaimRecovered,
    /// The claim holder declared further goals this work serves (ADR-0041).
    /// Logged because a task that grew its scope must show when and by whom;
    /// a wider claim with no history is indistinguishable from a rewritten one.
    CoverageDeclared,
    /// A conformance verdict moved the task out of `claimed` (ADR-0009).
    ConformanceRecorded,
    /// A human accepted work out of `in_review`, overruling the verdict.
    Resolved,
    /// The claim holder consented to move their own live claim from a
    /// superseded clause onto its active same-slug successor (ADR-0109).
    /// Distinct from the amendment's own automatic carry-forward
    /// (`reconnect_superseded_clauses`, which never touches a live claim):
    /// this is the one door the holder alone may open for themselves.
    ClauseReconnected,
}

impl TaskEventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskEventKind::Imported => "imported",
            TaskEventKind::Created => "created",
            TaskEventKind::Claimed => "claimed",
            TaskEventKind::LeaseRenewed => "lease_renewed",
            TaskEventKind::Released => "released",
            TaskEventKind::Blocked => "blocked",
            TaskEventKind::Reopened => "reopened",
            TaskEventKind::Abandoned => "abandoned",
            TaskEventKind::Questioned => "questioned",
            TaskEventKind::Answered => "answered",
            TaskEventKind::Paused => "paused",
            TaskEventKind::Resumed => "resumed",
            TaskEventKind::ClaimRecovered => "claim_recovered",
            TaskEventKind::CoverageDeclared => "coverage_declared",
            TaskEventKind::ConformanceRecorded => "conformance_recorded",
            TaskEventKind::Resolved => "resolved",
            TaskEventKind::ClauseReconnected => "clause_reconnected",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "imported" => Some(TaskEventKind::Imported),
            "created" => Some(TaskEventKind::Created),
            "claimed" => Some(TaskEventKind::Claimed),
            "lease_renewed" => Some(TaskEventKind::LeaseRenewed),
            "released" => Some(TaskEventKind::Released),
            "blocked" => Some(TaskEventKind::Blocked),
            "reopened" => Some(TaskEventKind::Reopened),
            "abandoned" => Some(TaskEventKind::Abandoned),
            "questioned" => Some(TaskEventKind::Questioned),
            "answered" => Some(TaskEventKind::Answered),
            "paused" => Some(TaskEventKind::Paused),
            "resumed" => Some(TaskEventKind::Resumed),
            "claim_recovered" => Some(TaskEventKind::ClaimRecovered),
            "coverage_declared" => Some(TaskEventKind::CoverageDeclared),
            "conformance_recorded" => Some(TaskEventKind::ConformanceRecorded),
            "resolved" => Some(TaskEventKind::Resolved),
            "clause_reconnected" => Some(TaskEventKind::ClauseReconnected),
            _ => None,
        }
    }
}

/// One appended record in the task lifecycle log (ADR-0064).
///
/// `after` is the task as it stood once the transition had been applied.
/// Replaying the log in `seq` order and assigning each `after` reproduces the
/// `tasks` table exactly, which is what makes the projection checkable rather
/// than merely believed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEvent {
    /// Total order of application. Assigned by the database, never by a caller.
    pub seq: i64,
    pub task_id: String,
    pub kind: TaskEventKind,
    /// The agent that caused this, where a transition has an actor. Genesis
    /// imports and predecessor-driven unblocking do not.
    pub actor: Option<String>,
    /// Unix seconds, supplied by the caller. Nothing here reads a clock: a
    /// projector that did could not replay deterministically (ADR-0064).
    pub recorded_at: i64,
    /// The task after the transition.
    pub after: Task,
    /// Transition-specific context as JSON: reason, question text, lease
    /// seconds. Empty when the transition carries none.
    pub detail: String,
}

/// The continuity of a task's current evidence window, derived from the log
/// (ADR-0064 decision 6).
///
/// ADR-0048 says a window survives a lapse so earlier work stays provable, but
/// a discontinuous window cannot certify itself as aligned. This is that
/// continuity, computed from the recorded transitions rather than carried as a
/// running total on the task row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimWindow {
    /// When the current window opened, if the task is in one.
    pub started_at: Option<i64>,
    /// How many times the lease lapsed inside this window.
    pub lapses: i64,
    /// Seconds of this window spent under no lease.
    pub unleased_seconds: i64,
    /// The window this one replaced, if it replaced one.
    ///
    /// A window is identified by its owner and the instant it opened, so a new
    /// one begins whenever either changes — including when an agent's *id*
    /// changes underneath a single session, which is how a live process running
    /// a superseded binary silently reset a window and reported `lapses: 0` as
    /// if nothing had happened.
    ///
    /// Counters reset with the window, correctly: the previous window's holes
    /// are not this window's. But that made a replacement indistinguishable
    /// from a first claim, which is the one thing a reader most needs to tell
    /// apart — work committed under the earlier window falls outside this one
    /// and cannot be certified, and nothing said so. `None` means this window
    /// is the task's first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced: Option<ReplacedWindow>,
}

/// What a current window replaced, for a reader deciding whether earlier work
/// can still be proved.
///
/// Deliberately carries the previous window's identity and its holes rather
/// than a bare flag: "there was an earlier window" prompts the question this
/// answers, which is whose it was and when it ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplacedWindow {
    /// Who held the previous window. `None` for a window that was open with no
    /// recorded owner, which the log permits.
    pub owner: Option<String>,
    /// When the previous window opened.
    pub started_at: Option<i64>,
    /// Lapses accumulated in the previous window, which did not travel here.
    pub lapses: i64,
    /// Seconds the previous window spent under no lease.
    pub unleased_seconds: i64,
    /// Whether the owner changed. A same-owner replacement is an ordinary
    /// re-claim after release; a *different* owner mid-task is either a
    /// deliberate handover or the identity collapse this field exists to make
    /// visible, and the reader needs to know which question to ask.
    pub owner_changed: bool,
}

impl ClaimWindow {
    /// A window with no holes in it. Not the same as "no window": a task that
    /// was never claimed and a task claimed once without lapsing are both
    /// continuous, and neither is capped by ADR-0048.
    ///
    /// Deliberately unchanged by `replaced`. Replacing a window is legitimate —
    /// a release and re-claim, a recorded handover — so treating it as
    /// discontinuous would refuse work that ADR-0048 permits. This reports the
    /// fact; whether it is a problem is the reader's call.
    pub fn is_continuous(&self) -> bool {
        self.lapses == 0
    }
}

/// A task row: a unit of claimable work serving a goal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub goal_id: String,
    pub parent_task_id: Option<String>,
    pub title: String,
    pub acceptance: String,
    pub status: TaskStatus,
    pub owner: Option<String>,
    pub claim_started_at: Option<i64>,
    pub lease_expires_at: Option<i64>,
    // The continuity of the current evidence window (ADR-0048) used to live
    // here as `claim_lapses` and `unleased_seconds`. It is derived from the
    // task log instead (ADR-0064 d5/d6): ask `claim_window`.
    //
    // Deliberately not kept as derived fields on this struct. Zero lapses means
    // "this window may certify itself as aligned", so a field that any read
    // path could leave unpopulated would fail *open* — quietly handing out a
    // clean receipt for work with holes in it. There is no field to forget.
    pub blocked_by: Option<String>,
    /// The branch this task's current evidence window is being done on
    /// (ADR-0057), joined at claim time from what the claiming session already
    /// declared to `open_session`. Nobody is asked to declare anything new, and
    /// `None` records honestly that the session declared no branch rather than
    /// guessing one — the server never inspects Git (ADR-0044).
    ///
    /// It follows the evidence window, not the agent: a same-owner re-claim
    /// keeps it, exactly as `claim_started_at` does, so it still names the
    /// branch the window's work was done on even if that agent has since moved
    /// on. A claim by a different owner opens a fresh window and re-reads it.
    pub branch: Option<String>,
    /// When the task was parked (needs_input/paused); after a bounded grace it
    /// becomes reclaimable by the pool so a vanished owner cannot strand it.
    pub parked_at: Option<i64>,
    /// Who accepted this task out of `in_review`, when, and the conformance
    /// record they overrode. A resolution is a human judgement that outranks an
    /// evidence-backed verdict, so it has to be at least as resolvable as the
    /// verdict it replaces — an acceptance nobody can attribute is narration,
    /// which is what the evidence chain exists to replace. `None` on rows
    /// resolved before this was recorded; that gap is not reconstructable.
    pub resolved_by: Option<String>,
    pub resolved_at: Option<i64>,
    pub resolved_conformance_id: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

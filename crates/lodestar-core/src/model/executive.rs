//! Executive types: tasks, their lifecycle events, claims and the scopes and
//! overlaps that coordinate concurrent agents.

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimWindow {
    /// When the current window opened, if the task is in one.
    pub started_at: Option<i64>,
    /// How many times the lease lapsed inside this window.
    pub lapses: i64,
    /// Seconds of this window spent under no lease.
    pub unleased_seconds: i64,
}

impl ClaimWindow {
    /// A window with no holes in it. Not the same as "no window": a task that
    /// was never claimed and a task claimed once without lapsing are both
    /// continuous, and neither is capped by ADR-0048.
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

/// Optional paths and symbol ids an agent declares when claiming work
/// (ADR-0024). Paths are normalized workspace-relative glob patterns; symbols
/// are opaque MindLeak `symbol:` ids.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskScope {
    pub paths: Vec<String>,
    pub symbols: Vec<String>,
}

/// How much an intersecting claim actually costs, from the branches the two
/// sessions declared (ADR-0035 heuristic 4).
///
/// An intersection is not one risk. Two agents editing a path on the same branch
/// are colliding *now*; on different branches they are building a merge conflict
/// for later. Reporting both as "overlap" is what made the advice easy to
/// ignore, because the caller had to guess which one it had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlapSignal {
    /// Both sessions declared the same branch: the edits land in one history.
    SameBranchCollision,
    /// The sessions declared different branches: divergence, paid at merge.
    CrossBranchMergeRisk,
    /// At least one side declared no branch, so the distinction is unknown.
    /// Declared context is self-reported and optional (ADR-0035 decision 5);
    /// absence degrades the signal, and must never be read as either verdict.
    Undeclared,
}

impl OverlapSignal {
    /// Classify one intersection from the two declared branches.
    pub fn classify(requester: Option<&str>, owner: Option<&str>) -> Self {
        match (requester, owner) {
            (Some(requester), Some(owner)) if requester == owner => Self::SameBranchCollision,
            (Some(_), Some(_)) => Self::CrossBranchMergeRisk,
            _ => Self::Undeclared,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SameBranchCollision => "same_branch_collision",
            Self::CrossBranchMergeRisk => "cross_branch_merge_risk",
            Self::Undeclared => "undeclared",
        }
    }
}

/// One active claim whose declared scope intersects a pre-flight request.
/// Advisory only: this reports ownership intent and never grants a lock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimOverlap {
    pub task_id: String,
    pub owner: String,
    pub lease_expires_at: i64,
    pub scope: TaskScope,
    pub matching_paths: Vec<String>,
    pub matching_symbols: Vec<String>,
    /// The branch the owning session declared at `open_session`, if any.
    pub owner_branch: Option<String>,
    pub signal: OverlapSignal,
}

/// The result of one pre-flight overlap check.
///
/// `requester_branch` is the branch the *asking* session declared, echoed back
/// because it is half of every `signal`. Without it an `undeclared` result is
/// ambiguous — the caller cannot tell whether the peer said nothing or it did
/// itself — and a stale declaration of its own stays invisible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimOverlapReport {
    pub requester_branch: Option<String>,
    pub claims: Vec<ClaimOverlap>,
}

/// One active claim as reported by a federated repository's Ackplane claim
/// registry (ADR-0096 clause 5) — the federated counterpart to reading a row
/// from the local `tasks`/`task_scopes` tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederatedClaim {
    pub task_id: String,
    pub owner: String,
    /// The branch Ackplane recorded for the owner, if any.
    pub owner_branch: Option<String>,
    pub lease_expires_at: i64,
    pub paths: Vec<String>,
    pub symbols: Vec<String>,
}

/// Where `check_federated_claim_overlap` reads active claims from when a
/// repository's coordination mode is federated (ADR-0096 clause 5).
///
/// A seam, not an implementation: `lodestar-core` stays local and
/// stdio-only (ADR-0004), so the concrete Ackplane RPC client lives outside
/// this crate. Tests inject a fixed in-memory implementation; a real one is
/// wired in by whichever binary composes this store with a live client.
pub trait FederatedClaimSource: Send + Sync {
    /// Every currently-active claim in the federated repository, excluding
    /// `exclude_task_id` if given. Ackplane decides what "active" means
    /// (whether a lease has actually expired); this seam does not filter or
    /// second-guess that answer.
    fn active_claims(&self, exclude_task_id: Option<&str>) -> crate::Result<Vec<FederatedClaim>>;
}

/// The authoritative claim state Ackplane granted for a `delegate`/`renew`/
/// `recover` request (ADR-0096 clause 3) — exactly the fields a
/// cache-projection copies into the local task row so `board`/`next`/`scope`
/// keep reading local, instant data afterward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FederatedClaimGrant {
    pub owner: String,
    /// The branch Ackplane recorded for the owner, if any.
    pub branch: Option<String>,
    pub claim_started_at: i64,
    pub lease_expires_at: i64,
    pub claim_lapses: i64,
    pub paths: Vec<String>,
    pub symbols: Vec<String>,
}

/// One `delegate`/`renew`/`recover` round trip's outcome against Ackplane's
/// claim CAS.
///
/// Not the same distinction as `Result`: a rejection is Ackplane's arbiter
/// answering and refusing (someone else holds the task, a renew's owner
/// mismatches, ...), which is a normal CAS outcome exactly like a local
/// `claim_task_with_partial_scope` returning `Ok(false)`. A transport or
/// protocol failure — the arbiter did not answer at all — is not represented
/// here; it is the `Err` side of the `Result` this outcome is wrapped in, so
/// it can never be confused with a business refusal (ADR-0096 clause 3's
/// "actionable typed refusal").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FederatedClaimOutcome {
    Granted(FederatedClaimGrant),
    Rejected { diagnostic: String },
}

/// Where Ackplane's claim CAS is asked to decide `claim`/`renew`/`release`/
/// `recover` for a federated repository (ADR-0096 clauses 2-4, 6) instead of
/// the local `tasks` table deciding them.
///
/// A seam, not an implementation, matching [`FederatedClaimSource`]: this
/// crate stays local and stdio-only (ADR-0004), so the concrete
/// authenticated Ackplane RPC client — and the blocking bridge a synchronous
/// call site needs for it — lives outside this crate. Tests inject a fixed
/// implementation; a real one is wired in by whichever binary composes this
/// store with a live client.
pub trait FederatedClaimAuthority: Send + Sync {
    /// `paths`/`symbols` are the full scope to request, already resolved by
    /// the caller from any partial declaration — the wire contract has no
    /// "leave scope alone" value, unlike the local partial-scope call.
    fn delegate(
        &self,
        task_id: &str,
        owner: &str,
        branch: Option<&str>,
        lease_secs: i64,
        paths: &[String],
        symbols: &[String],
    ) -> crate::Result<FederatedClaimOutcome>;

    fn renew(
        &self,
        task_id: &str,
        owner: &str,
        lease_secs: i64,
    ) -> crate::Result<FederatedClaimOutcome>;

    /// `true` iff a live lease was actually holed. Ackplane holes the lease
    /// rather than deleting the row (ADR-0096 clause 6), so a release of an
    /// already-expired or foreign claim is a no-op, exactly like the local
    /// `release_task`.
    fn release(&self, task_id: &str, owner: &str) -> crate::Result<bool>;

    fn recover(
        &self,
        request: &FederatedClaimRecoverRequest,
    ) -> crate::Result<FederatedClaimOutcome>;
}

/// Everything [`FederatedClaimAuthority::recover`] needs, bundled to keep the
/// method's own argument count sane (mirrors `ackplane-server`'s
/// `ClaimRecoverRequest` doing the same for the wire-level equivalent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FederatedClaimRecoverRequest {
    pub task_id: String,
    pub expected_owner: String,
    pub owner: String,
    pub branch: Option<String>,
    pub reason: String,
    pub lease_secs: i64,
    pub paths: Vec<String>,
    pub symbols: Vec<String>,
}

/// One durable, append-only entry in a task's dialogue thread (ADR-0020,
/// ADR-0046): a `needs_input` question from the owning agent, its `answer`, or
/// a `note` recording why a state change parked or blocked the work.
///
/// `audience` is the agent id a question is addressed to; `None` means a human.
/// It is the only addressing in the system, and it routes nothing — an addressed
/// question is a durable row a peer discovers by asking, never a message pushed
/// at it (ADR-0046).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskQa {
    pub id: i64,
    pub task_id: String,
    pub kind: String,
    pub body: String,
    pub author: String,
    pub audience: Option<String>,
    pub created_at: i64,
}

/// One unanswered question addressed at a human, with enough context to answer
/// it without another lookup.
///
/// A human has no agent id, so this cannot be a `TaskQa` from
/// `pending_questions` — `audience IS NULL` *is* the addressing (ADR-0046
/// clause 2), and a query that matches an id can never return one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HumanQuestion {
    pub question_id: i64,
    pub task_id: String,
    pub task_title: String,
    /// The agent that parked the task asking.
    pub asked_by: String,
    pub question: String,
    pub asked_at: i64,
    /// How long it has gone unanswered. Reported, never judged: a staleness
    /// threshold invented here would become a policy nobody agreed to.
    pub waiting_seconds: i64,
}

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

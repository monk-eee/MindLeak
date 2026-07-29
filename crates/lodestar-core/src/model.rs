//! Domain model for the Lodestar Intent Plane: goals (the constitution), tasks
//! (the executive), conformance verdicts, and consolidated learned knowledge.

use serde::{Deserialize, Serialize};

/// What a goal expresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GoalKind {
    /// A thing to achieve.
    Objective,
    /// A boundary that must hold.
    Constraint,
    /// A load-bearing rule that must never be violated.
    Invariant,
    /// A broad decision rule. Normative but ambiguous cases route to review,
    /// never an automatic hard block (SPEC-CONSTITUTION §4).
    Principle,
}

impl GoalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            GoalKind::Objective => "objective",
            GoalKind::Constraint => "constraint",
            GoalKind::Invariant => "invariant",
            GoalKind::Principle => "principle",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "objective" => Some(GoalKind::Objective),
            "constraint" => Some(GoalKind::Constraint),
            "invariant" => Some(GoalKind::Invariant),
            "principle" => Some(GoalKind::Principle),
            _ => None,
        }
    }

    /// Constraints and invariants are what conformance checks against.
    pub fn is_normative(&self) -> bool {
        matches!(self, GoalKind::Constraint | GoalKind::Invariant)
    }
}

/// The proportional outcome when a clause is not met (SPEC-CONSTITUTION §4/§8):
/// uncertainty asks for review, only a specific active clause with adequate
/// evidence can hard-block. Ordered by severity: `advise < review < block`, so
/// the ADR-0034 ceiling rule can take a minimum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Consequence {
    /// Surface guidance; never blocks.
    Advise,
    /// Route to human review.
    Review,
    /// Hard policy; may block with adequate evidence.
    Block,
}

impl Consequence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Consequence::Advise => "advise",
            Consequence::Review => "review",
            Consequence::Block => "block",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "advise" => Some(Consequence::Advise),
            "review" => Some(Consequence::Review),
            "block" => Some(Consequence::Block),
            _ => None,
        }
    }
}

/// Where a clause came from (SPEC-CONSTITUTION §10 `ClauseSource.origin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClauseOrigin {
    /// Authored directly in this project.
    Local,
    /// Adopted from an immutable policy pack.
    Pack,
    /// Derived from a cited repository fact.
    Discovered,
}

impl ClauseOrigin {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClauseOrigin::Local => "local",
            ClauseOrigin::Pack => "pack",
            ClauseOrigin::Discovered => "discovered",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "local" => Some(ClauseOrigin::Local),
            "pack" => Some(ClauseOrigin::Pack),
            "discovered" => Some(ClauseOrigin::Discovered),
            _ => None,
        }
    }
}

/// Lifecycle of a goal version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GoalStatus {
    Draft,
    Active,
    Superseded,
}

impl GoalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            GoalStatus::Draft => "draft",
            GoalStatus::Active => "active",
            GoalStatus::Superseded => "superseded",
        }
    }

    pub fn from_tag(s: &str) -> Option<Self> {
        match s {
            "draft" => Some(GoalStatus::Draft),
            "active" => Some(GoalStatus::Active),
            "superseded" => Some(GoalStatus::Superseded),
            _ => None,
        }
    }
}

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
    /// A conformance verdict moved the task out of `claimed` (ADR-0009).
    ConformanceRecorded,
    /// A human accepted work out of `in_review`, overruling the verdict.
    Resolved,
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
            TaskEventKind::ConformanceRecorded => "conformance_recorded",
            TaskEventKind::Resolved => "resolved",
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
            "conformance_recorded" => Some(TaskEventKind::ConformanceRecorded),
            "resolved" => Some(TaskEventKind::Resolved),
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

/// The outcome of a conformance check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The change is sanctioned and consistent with governing intent.
    Aligned,
    /// Governed code changed without a covering task (unsanctioned).
    Drift,
    /// The change contradicts a constraint/invariant.
    Violation,
    /// A semantic check could not decide; a human should look.
    NeedsHuman,
}

/// How an active goal governs a linked MindLeak code node (ADR-0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodeBindingMode {
    Governed,
    ForbidChange,
}

impl CodeBindingMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            CodeBindingMode::Governed => "governed",
            CodeBindingMode::ForbidChange => "forbid_change",
        }
    }

    pub fn from_tag(value: &str) -> Option<Self> {
        match value {
            "governed" => Some(CodeBindingMode::Governed),
            "forbid_change" => Some(CodeBindingMode::ForbidChange),
            _ => None,
        }
    }
}

/// An active goal plus the policy governing one linked code node.
#[derive(Debug, Clone, Serialize)]
pub struct CodeBinding {
    pub goal: Goal,
    pub mode: CodeBindingMode,
}

/// The forward-looking disposition returned by `advise` (ADR-0029): a
/// proportional judgment made *before* work is done, from clause resolution
/// alone. It is deliberately not a `Verdict` — advice never records a
/// conformance result and never runs the semantic judge, so it can only surface
/// what governs the intended change, warn about a would-be drift, block on a
/// hard `forbid_change` lock, or defer to a human when no constitution exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdviceDisposition {
    /// Nothing blocks the change; any in-scope governing clauses are surfaced to honour.
    Advise,
    /// The change would drift outside a covering task; get a covering task or review first.
    Review,
    /// A hard `forbid_change` clause locks this code; do not proceed without a waiver.
    Block,
    /// No constitution is adopted (or policy is genuinely ambiguous); a human should look.
    NeedsHuman,
}

impl AdviceDisposition {
    /// The stable snake_case tag, matching the serialized form.
    pub fn as_str(&self) -> &'static str {
        match self {
            AdviceDisposition::Advise => "advise",
            AdviceDisposition::Review => "review",
            AdviceDisposition::Block => "block",
            AdviceDisposition::NeedsHuman => "needs_human",
        }
    }
}

/// One active clause governing a node in an intended change scope (ADR-0029).
#[derive(Debug, Clone, Serialize)]
pub struct GoverningClause {
    pub node_id: String,
    pub goal: Goal,
    pub mode: CodeBindingMode,
}

/// The result of `advise` (ADR-0029): the active clauses governing an intended
/// change scope plus one proportional disposition. It carries no evidence and
/// records no verdict — a read-only projection of the adopted constitution.
#[derive(Debug, Clone, Serialize)]
pub struct Advice {
    pub disposition: AdviceDisposition,
    pub governing: Vec<GoverningClause>,
    pub findings: Vec<String>,
}

/// One MindLeak graph fact supporting an evidence claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceProvenance {
    pub source_id: String,
    pub target_id: String,
    pub relation: String,
}

/// Versioned evidence received across the loose MindLeak/Lodestar seam.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConformanceEvidence {
    pub schema_version: u32,
    pub task_id: Option<String>,
    pub agent_id: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub changed_node_ids: Vec<String>,
    pub failed_node_ids: Vec<String>,
    pub execution_ids: Vec<String>,
    pub successful_execution_ids: Vec<String>,
    pub commit_ids: Vec<String>,
    pub summary: String,
    pub provenance: Vec<EvidenceProvenance>,
}

impl Verdict {
    pub fn as_str(&self) -> &'static str {
        match self {
            Verdict::Aligned => "aligned",
            Verdict::Drift => "drift",
            Verdict::Violation => "violation",
            Verdict::NeedsHuman => "needs_human",
        }
    }

    pub fn from_tag(value: &str) -> Option<Self> {
        match value {
            "aligned" => Some(Verdict::Aligned),
            "drift" => Some(Verdict::Drift),
            "violation" => Some(Verdict::Violation),
            "needs_human" => Some(Verdict::NeedsHuman),
            _ => None,
        }
    }
}

/// A one-glance summary of the conformance record a task closed on.
///
/// A task reaching `done` says nothing about whether its evidence ever affirmed
/// the work. Measured over this repository, 57 of 101 `done` tasks rested on a
/// `drift`/`needs_human` verdict or on an `aligned` one covering no nodes at
/// all, and every one of them read on the board exactly like a task whose
/// evidence proved something. `affirms` is the distinction, carried where the
/// completion is reported rather than left for a reader to reconstruct from the
/// conformance chain.
///
/// Derived at read time from the durable record; nothing here is stored twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TaskReceipt {
    /// The conformance record this summarises, resolvable after the fact.
    pub conformance_id: i64,
    pub verdict: Verdict,
    /// How many nodes the evidence bundle actually covered.
    pub covered_nodes: usize,
    pub checked_at: i64,
    /// Whether the receipt affirmed the work: `aligned` **and** covering at
    /// least one node. An `aligned` verdict over an empty bundle is agreement
    /// about nothing, which is not the same as proof.
    pub affirms: bool,
}

/// One persisted conformance audit record: the durable, resolvable evidence
/// link for a task. Its `id` is stable and addressable after the fact, and the
/// stored `evidence` is exactly the bundle that produced `verdict`/`findings`.
#[derive(Debug, Clone, Serialize)]
pub struct ConformanceRecord {
    pub id: i64,
    pub task_id: Option<String>,
    pub evidence_schema_version: u32,
    pub evidence: String,
    pub verdict: Verdict,
    pub findings: String,
    pub checked_at: i64,
}

/// An authoritative conformance preflight. `complete_task` consumes this exact
/// persisted result and rejects it if the evidence or relevant intent state has
/// changed, so an optional semantic judge is never invoked twice for one task
/// transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConformanceCheck {
    pub id: i64,
    pub token: String,
    pub verdict: Verdict,
    pub findings: Vec<String>,
}

/// One immutable constitutional version: the frozen preamble and clause set
/// that authorises verdicts (SPEC-CONSTITUTION §10). An amendment writes a new
/// version; prior conformance records retain the version they were judged
/// under. Migration does not invent a purpose, preamble, or authority.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstitutionVersion {
    pub id: String,
    pub version: i64,
    pub project_identity: Option<String>,
    pub purpose: Option<String>,
    pub preamble: Option<String>,
    pub status: GoalStatus,
    pub created_by: Option<String>,
    pub created_at: i64,
    pub activated_by: Option<String>,
    pub activated_at: Option<i64>,
}

/// Whether a project has adopted a constitution at all
/// (SPEC-CONSTITUTION §11). Reported rather than inferred, so an agent can tell
/// "no policy exists" apart from "policy exists and permits this".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConstitutionState {
    /// No constitutional version exists; conformance can only defer to a human.
    Absent,
    /// A version is drafted but not activated, so it authorises nothing yet.
    Draft,
    /// An activated version governs work.
    Active,
}

impl ConstitutionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConstitutionState::Absent => "absent",
            ConstitutionState::Draft => "draft",
            ConstitutionState::Active => "active",
        }
    }
}

/// The adoption state of the local constitution: which lifecycle stage it is
/// in, the version that stage refers to, and how many clauses it carries. A
/// draft reports its own clause count so bootstrap progress is visible without
/// implying the clauses are enforceable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstitutionStatus {
    pub state: ConstitutionState,
    pub version: Option<ConstitutionVersion>,
    pub clause_count: i64,
}

/// A bootstrap proposal (SPEC-CONSTITUTION 7.3): a drafted version, the cited
/// repository facts grounding it, and the Common Core clauses awaiting an
/// adopt/tailor/reject disposition. Nothing here governs anything — the draft
/// authorises no verdict until it is explicitly activated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstitutionProposal {
    pub version: ConstitutionVersion,
    pub facts: Vec<crate::discovery::ProjectFact>,
    pub common_core: crate::policy::PackProposalBatch,
}

/// A goal row: a clause of the constitution (SPEC-CONSTITUTION §10). The
/// enforcement fields (`scope`, `evidence_contract`, `consequence`) stay absent
/// until explicitly completed; an incomplete clause is review-only and can
/// never drive a hard verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub slug: String,
    pub kind: GoalKind,
    pub title: String,
    pub statement: String,
    pub status: GoalStatus,
    pub version: i64,
    pub parent_id: Option<String>,
    pub superseded_by: Option<String>,
    pub reason: Option<String>,
    pub created_at: i64,
    /// The constitutional version this clause belongs to, if any.
    pub constitution_version: Option<String>,
    /// Why the clause exists (distinct from `reason`, the amendment note).
    pub rationale: Option<String>,
    /// The declared scope in which the clause applies.
    pub scope: Option<String>,
    /// The evidence contract that satisfies the clause.
    pub evidence_contract: Option<String>,
    /// The proportional consequence of non-conformance.
    pub consequence: Option<Consequence>,
    /// Whether a bounded waiver may suspend the clause.
    pub waivable: bool,
    /// The authority required to waive the clause.
    pub waiver_authority: Option<String>,
    /// The provenance of the clause.
    pub origin: ClauseOrigin,
}

impl Goal {
    /// A clause can drive a hard verdict only once it declares a scope, an
    /// evidence contract, and a consequence. Until then it is review-only
    /// (SPEC-CONSTITUTION §10: incomplete clauses guide review, never block).
    pub fn is_enforceable(&self) -> bool {
        self.scope.is_some() && self.evidence_contract.is_some() && self.consequence.is_some()
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

/// A learned-knowledge row: a consolidated regularity with provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Knowledge {
    pub id: String,
    pub statement: String,
    pub evidence: String,
    pub weight: f64,
    pub half_life_hours: f64,
    pub confirmed_at: i64,
    pub created_at: i64,
}

impl Knowledge {
    /// The MindLeak node ids this knowledge was consolidated from, parsed
    /// best-effort from the stored `evidence` JSON (`{"nodes": [...]}`). Empty
    /// when the evidence is hand-authored or not in that shape — so a hand-written
    /// note never accidentally governs conformance.
    pub fn referenced_nodes(&self) -> Vec<String> {
        serde_json::from_str::<serde_json::Value>(&self.evidence)
            .ok()
            .and_then(|value| {
                value
                    .get("nodes")
                    .and_then(|nodes| nodes.as_array())
                    .map(|nodes| {
                        nodes
                            .iter()
                            .filter_map(|node| node.as_str().map(str::to_string))
                            .collect()
                    })
            })
            .unwrap_or_default()
    }
}

/// An opaque proven-signal candidate handed across the loose MindLeak → Lodestar
/// seam for gated promotion (ADR-0022). `evidence_node_ids` are MindLeak node ids
/// treated as opaque strings; the span comes from edge provenance. `statement`,
/// when present, is a pre-distilled summary (e.g. from a local model); when
/// absent the promoter builds a deterministic templated statement, so promotion
/// never depends on an LLM.
#[derive(Debug, Clone)]
pub struct SignalPromotion {
    pub subject: String,
    pub evidence_node_ids: Vec<String>,
    pub first_seen: i64,
    pub last_seen: i64,
    pub statement: Option<String>,
}

/// The result of a conformance check (returned to callers; also audited).
#[derive(Debug, Clone, Serialize)]
pub struct ConformanceResult {
    pub verdict: Verdict,
    pub findings: Vec<String>,
}

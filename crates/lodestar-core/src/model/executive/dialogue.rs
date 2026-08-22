//! A task's durable dialogue: questions, answers, and notes; and the human
//! inbox that surfaces the unanswered ones.

use serde::{Deserialize, Serialize};

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

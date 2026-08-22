//! Pure data types returned by a telemetry snapshot.

use serde::Serialize;
use serde_json::Value;

/// Aggregate metrics for one event `name` (e.g. a tool).
///
/// `calls`/`errors` are **lifetime** totals over the whole append-only trail —
/// they never shrink, so a single historical failure keeps `errors >= 1`
/// forever. Current health is a *separate* question answered by append order: a
/// tool whose most recently recorded event succeeded is healthy even though its
/// lifetime `errors` is non-zero. `currently_failing` is the derived verdict;
/// the timestamps provide operator context, and `last_error_detail` keeps the
/// historical failure queryable even after the raw event scrolls out of the
/// bounded `recent` window.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NameMetric {
    pub name: String,
    pub calls: i64,
    pub errors: i64,
    pub total_ms: i64,
    pub min_ms: i64,
    pub max_ms: i64,
    pub avg_ms: f64,
    /// Timestamp of this tool's most recent successful (`ok`) event, if any.
    pub last_success_at: Option<i64>,
    /// Timestamp of this tool's most recent error event, if any.
    pub last_error_at: Option<i64>,
    /// The `detail` payload of the most recent error, retained as an audit path
    /// even once the event ages out of the bounded `recent` window.
    pub last_error_detail: Option<Value>,
    /// Timestamp and detail of the most recent deliberately skipped event.
    /// Retained after it leaves `recent`, because transition-only maintenance
    /// telemetry must stay actionable without repeating the same event.
    pub last_degraded_at: Option<i64>,
    pub last_degraded_detail: Option<Value>,
    /// Derived from append order: the tool's most recent event was an error. A
    /// resolved historical error is `false`, so the pane never presents a fixed
    /// fault as an active one, including when calls share a timestamp.
    pub currently_failing: bool,
    /// The tool's most recent event was deliberately skipped. Optional model
    /// maintenance uses this state so deterministic graph health stays green
    /// while the unavailable enrichment remains visible.
    pub currently_degraded: bool,
}

/// One recorded event, as returned by a snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct EventRow {
    pub ts: i64,
    pub kind: String,
    pub name: String,
    pub outcome: String,
    pub duration_ms: Option<i64>,
    pub detail: Option<Value>,
}

/// Whether one registered session read memory before its first attributed
/// write. Derived from the append-only audit trail; never stored separately.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MemoryHabit {
    pub agent_id: String,
    pub memory_reads: i64,
    pub attributed_writes: i64,
    /// `None` means the session has not written yet, so there is no verdict.
    pub read_before_first_write: Option<bool>,
}

/// One high-volume read path called out by the usage retrospective.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UsageMetric {
    pub name: String,
    pub calls: i64,
    pub total_ms: i64,
}

/// Deterministic interpretation of the telemetry already loaded for a snapshot.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UsageRetrospective {
    pub background_read_calls: i64,
    pub preflight_read_calls: i64,
    pub architectural_decision_calls: i64,
    pub writing_sessions: i64,
    pub writing_sessions_without_memory_read: i64,
    pub high_volume_reads: Vec<UsageMetric>,
    pub recommendations: Vec<String>,
}

/// A point-in-time view of recorded observability.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    /// Lifetime count of every recorded event.
    pub total_events: i64,
    /// Lifetime count of error events. Never shrinks — this is history, not the
    /// current fault state. Use `currently_failing_tools` for live health.
    pub total_errors: i64,
    /// How many tools are failing *right now* (their most recent event errored).
    /// Derived from `by_name`; distinct from the lifetime `total_errors` tally.
    pub currently_failing_tools: i64,
    /// How many tools are degraded right now (their most recent event skipped).
    pub currently_degraded_tools: i64,
    pub by_name: Vec<NameMetric>,
    pub recent: Vec<EventRow>,
    /// Most recently active registered sessions, newest first and bounded.
    pub memory_habits: Vec<MemoryHabit>,
    /// Factual interpretation of lifetime call volume and the bounded
    /// 10,000-event / 32-session memory-habit sample.
    pub retrospective: UsageRetrospective,
}

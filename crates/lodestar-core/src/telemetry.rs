//! Observability for Lodestar's own model calls (decompose/judge/draft_question).
//!
//! Mirrors `mindleak_core::telemetry`'s shape where it makes sense (an
//! append-only events table, a point-in-time `Snapshot`), but is its own table
//! with no coupling to the Constitution/task schema: a model call is not a
//! coordination event. Recording is best-effort -- a write failure is logged
//! and swallowed so instrumentation can never change the result of the
//! decompose/judge/draft_question call it observes.

use rusqlite::{params, Connection};
use serde::Serialize;

use crate::error::Result;

/// Create the model-call telemetry table if it does not yet exist.
pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS model_call_events (
            id                 INTEGER PRIMARY KEY AUTOINCREMENT,
            ts                 INTEGER NOT NULL,
            operation          TEXT    NOT NULL,
            outcome            TEXT    NOT NULL,
            duration_ms        INTEGER,
            model              TEXT,
            prompt_tokens      INTEGER,
            completion_tokens  INTEGER,
            total_tokens       INTEGER
         );
         CREATE INDEX IF NOT EXISTS idx_model_call_events_ts ON model_call_events(ts);",
    )?;
    Ok(())
}

/// Token usage reported by an OpenAI-compatible `/chat/completions` response's
/// `usage` field. Any or all of these are `None` when a local server omits the
/// field entirely (some do), never zero -- a zero would misreport "this call
/// cost nothing" rather than "this server did not say".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct TokenUsage {
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}

impl TokenUsage {
    /// Parse a chat-completion response's top-level `usage` object. `None` when
    /// the field is absent or not an object -- not an error, since `usage` is
    /// not part of the OpenAI API's required contract and several local
    /// servers omit it.
    pub fn from_response(value: &serde_json::Value) -> Option<Self> {
        let usage = value.get("usage")?.as_object()?;
        let field = |key: &str| usage.get(key).and_then(serde_json::Value::as_i64);
        Some(TokenUsage {
            prompt_tokens: field("prompt_tokens"),
            completion_tokens: field("completion_tokens"),
            total_tokens: field("total_tokens"),
        })
    }
}

/// Append one model-call event. Callers treat failures as non-fatal.
#[allow(clippy::too_many_arguments)]
pub fn record(
    conn: &Connection,
    ts: i64,
    operation: &str,
    outcome: &str,
    duration_ms: Option<i64>,
    model: Option<&str>,
    usage: Option<TokenUsage>,
) -> Result<()> {
    ensure_table(conn)?;
    let usage = usage.unwrap_or_default();
    conn.execute(
        "INSERT INTO model_call_events
            (ts, operation, outcome, duration_ms, model, prompt_tokens, completion_tokens, total_tokens)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            ts,
            operation,
            outcome,
            duration_ms,
            model,
            usage.prompt_tokens,
            usage.completion_tokens,
            usage.total_tokens,
        ],
    )?;
    Ok(())
}

/// Lifetime metrics for one recorded `operation` (e.g. `"decompose"`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OperationMetric {
    pub operation: String,
    pub calls: i64,
    pub errors: i64,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub total_tokens: i64,
    pub avg_ms: f64,
}

/// A point-in-time view of recorded model-call observability.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ModelCallSnapshot {
    pub total_calls: i64,
    pub total_errors: i64,
    pub total_prompt_tokens: i64,
    pub total_completion_tokens: i64,
    pub total_tokens: i64,
    pub by_operation: Vec<OperationMetric>,
}

/// Read the current snapshot. Returns a zeroed snapshot (never an error) when
/// the table does not exist yet -- no model call has ever been recorded is a
/// fact worth reporting, not a failure.
pub fn snapshot(conn: &Connection) -> Result<ModelCallSnapshot> {
    ensure_table(conn)?;
    let (total_calls, total_errors, total_prompt_tokens, total_completion_tokens, total_tokens) =
        conn.query_row(
            "SELECT
                COUNT(1),
                COALESCE(SUM(CASE WHEN outcome != 'ok' THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(prompt_tokens), 0),
                COALESCE(SUM(completion_tokens), 0),
                COALESCE(SUM(total_tokens), 0)
             FROM model_call_events",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )?;

    let mut stmt = conn.prepare(
        "SELECT
            operation,
            COUNT(1),
            SUM(CASE WHEN outcome != 'ok' THEN 1 ELSE 0 END),
            COALESCE(SUM(prompt_tokens), 0),
            COALESCE(SUM(completion_tokens), 0),
            COALESCE(SUM(total_tokens), 0),
            COALESCE(AVG(duration_ms), 0.0)
         FROM model_call_events
         GROUP BY operation
         ORDER BY operation",
    )?;
    let by_operation = stmt
        .query_map([], |row| {
            Ok(OperationMetric {
                operation: row.get(0)?,
                calls: row.get(1)?,
                errors: row.get(2)?,
                total_prompt_tokens: row.get(3)?,
                total_completion_tokens: row.get(4)?,
                total_tokens: row.get(5)?,
                avg_ms: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(ModelCallSnapshot {
        total_calls,
        total_errors,
        total_prompt_tokens,
        total_completion_tokens,
        total_tokens,
        by_operation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_conn() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn usage_parses_all_three_fields_when_present() {
        let value = serde_json::json!({
            "usage": { "prompt_tokens": 120, "completion_tokens": 40, "total_tokens": 160 }
        });
        assert_eq!(
            TokenUsage::from_response(&value),
            Some(TokenUsage {
                prompt_tokens: Some(120),
                completion_tokens: Some(40),
                total_tokens: Some(160),
            })
        );
    }

    #[test]
    fn usage_is_none_when_the_field_is_absent() {
        let value = serde_json::json!({ "choices": [] });
        assert_eq!(TokenUsage::from_response(&value), None);
    }

    /// A local server that reports usage as a differently-shaped value (a
    /// string, an array) must not be read as zero cost -- that would silently
    /// under-report spend rather than admitting the server didn't say.
    #[test]
    fn a_malformed_usage_field_is_treated_as_absent_not_zero() {
        let value = serde_json::json!({ "usage": "unlimited" });
        assert_eq!(TokenUsage::from_response(&value), None);
    }

    #[test]
    fn a_partial_usage_object_reports_only_the_fields_present() {
        let value = serde_json::json!({ "usage": { "total_tokens": 99 } });
        assert_eq!(
            TokenUsage::from_response(&value),
            Some(TokenUsage {
                prompt_tokens: None,
                completion_tokens: None,
                total_tokens: Some(99),
            })
        );
    }

    #[test]
    fn snapshot_on_an_empty_table_reports_zero_not_an_error() {
        let conn = memory_conn();
        let snapshot = snapshot(&conn).unwrap();
        assert_eq!(snapshot.total_calls, 0);
        assert_eq!(snapshot.total_tokens, 0);
        assert!(snapshot.by_operation.is_empty());
    }

    #[test]
    fn snapshot_aggregates_tokens_and_errors_per_operation() {
        let conn = memory_conn();
        record(
            &conn,
            1,
            "decompose",
            "ok",
            Some(500),
            Some("glm4:9b"),
            Some(TokenUsage {
                prompt_tokens: Some(100),
                completion_tokens: Some(20),
                total_tokens: Some(120),
            }),
        )
        .unwrap();
        record(
            &conn,
            2,
            "decompose",
            "error",
            Some(50),
            Some("glm4:9b"),
            None,
        )
        .unwrap();
        record(
            &conn,
            3,
            "judge",
            "ok",
            Some(300),
            Some("glm4:9b"),
            Some(TokenUsage {
                prompt_tokens: Some(50),
                completion_tokens: Some(10),
                total_tokens: Some(60),
            }),
        )
        .unwrap();

        let snapshot = snapshot(&conn).unwrap();
        assert_eq!(snapshot.total_calls, 3);
        assert_eq!(snapshot.total_errors, 1);
        assert_eq!(snapshot.total_tokens, 180);

        let decompose = snapshot
            .by_operation
            .iter()
            .find(|metric| metric.operation == "decompose")
            .unwrap();
        assert_eq!(decompose.calls, 2);
        assert_eq!(decompose.errors, 1);
        assert_eq!(decompose.total_tokens, 120);
    }
}

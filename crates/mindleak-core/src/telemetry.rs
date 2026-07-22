//! Observability audit trail and metrics (ADR-0010).
//!
//! Records every MCP tool invocation to a dedicated, append-only
//! `telemetry_events` table this module owns. The table is *not* graph state: it
//! never decays, never participates in traversal or pruning, and is created
//! idempotently so it adds no migration coupling. Recording is best-effort — a
//! write failure is logged and swallowed so instrumentation can never change the
//! result of the operation it observes.
//!
//! All real-time logging goes through `tracing` to **stderr**; stdout is
//! reserved for the JSON-RPC protocol.

use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;

use crate::error::Result;

/// Create the telemetry table and its index if they do not yet exist.
pub fn ensure_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS telemetry_events (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            ts          INTEGER NOT NULL,
            kind        TEXT    NOT NULL,
            name        TEXT    NOT NULL,
            outcome     TEXT    NOT NULL,
            duration_ms INTEGER,
            detail      TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_telemetry_ts ON telemetry_events(ts);",
    )?;
    Ok(())
}

/// Append one event. Callers treat failures as non-fatal.
pub fn record(
    conn: &Connection,
    ts: i64,
    kind: &str,
    name: &str,
    outcome: &str,
    duration_ms: Option<i64>,
    detail: Option<&Value>,
) -> Result<()> {
    ensure_table(conn)?;
    let detail_text = detail.map(|d| d.to_string());
    conn.execute(
        "INSERT INTO telemetry_events (ts, kind, name, outcome, duration_ms, detail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![ts, kind, name, outcome, duration_ms, detail_text],
    )?;
    Ok(())
}

/// Aggregate metrics for one event `name` (e.g. a tool).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NameMetric {
    pub name: String,
    pub calls: i64,
    pub errors: i64,
    pub total_ms: i64,
    pub min_ms: i64,
    pub max_ms: i64,
    pub avg_ms: f64,
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

/// A point-in-time view of recorded observability.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub total_events: i64,
    pub total_errors: i64,
    pub by_name: Vec<NameMetric>,
    pub recent: Vec<EventRow>,
}

/// Aggregate metrics plus the most recent `recent_limit` events.
pub fn snapshot(conn: &Connection, recent_limit: usize) -> Result<Snapshot> {
    ensure_table(conn)?;

    let mut by_name = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT name,
                    COUNT(*)                                        AS calls,
                    SUM(CASE WHEN outcome = 'error' THEN 1 ELSE 0 END) AS errors,
                    COALESCE(SUM(duration_ms), 0)                   AS total_ms,
                    COALESCE(MIN(duration_ms), 0)                   AS min_ms,
                    COALESCE(MAX(duration_ms), 0)                   AS max_ms
             FROM telemetry_events
             GROUP BY name
             ORDER BY calls DESC, name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let calls: i64 = row.get(1)?;
            let total_ms: i64 = row.get(3)?;
            Ok(NameMetric {
                name: row.get(0)?,
                calls,
                errors: row.get(2)?,
                total_ms,
                min_ms: row.get(4)?,
                max_ms: row.get(5)?,
                avg_ms: if calls > 0 {
                    total_ms as f64 / calls as f64
                } else {
                    0.0
                },
            })
        })?;
        for row in rows {
            by_name.push(row?);
        }
    }

    let total_events: i64 =
        conn.query_row("SELECT COUNT(*) FROM telemetry_events", [], |r| r.get(0))?;
    let total_errors: i64 = conn.query_row(
        "SELECT COUNT(*) FROM telemetry_events WHERE outcome = 'error'",
        [],
        |r| r.get(0),
    )?;

    let mut recent = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT ts, kind, name, outcome, duration_ms, detail
             FROM telemetry_events
             ORDER BY id DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map([recent_limit as i64], |row| {
            let detail: Option<String> = row.get(5)?;
            Ok(EventRow {
                ts: row.get(0)?,
                kind: row.get(1)?,
                name: row.get(2)?,
                outcome: row.get(3)?,
                duration_ms: row.get(4)?,
                detail: detail.and_then(|d| serde_json::from_str(&d).ok()),
            })
        })?;
        for row in rows {
            recent.push(row?);
        }
    }

    Ok(Snapshot {
        total_events,
        total_errors,
        by_name,
        recent,
    })
}

/// Install the process-wide `tracing` subscriber: **stderr only**, env-gated.
///
/// Safe to call once from a binary's `main`; a second call is a no-op. Reads
/// `MINDLEAK_LOG` (filter, default `info`) and `MINDLEAK_LOG_FORMAT`
/// (`pretty` | `json`, default `pretty`). Never writes to stdout.
pub fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_env("MINDLEAK_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let json = std::env::var("MINDLEAK_LOG_FORMAT")
        .map(|v| v.eq_ignore_ascii_case("json"))
        .unwrap_or(false);

    let builder = fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(true);

    // try_init is a no-op (Err) if a global subscriber is already set.
    let _ = if json {
        builder.json().try_init()
    } else {
        builder.try_init()
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn conn() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn snapshot_aggregates_calls_errors_and_latency() {
        let c = conn();
        record(&c, 100, "tool_call", "recall", "ok", Some(10), None).unwrap();
        record(&c, 101, "tool_call", "recall", "ok", Some(30), None).unwrap();
        record(
            &c,
            102,
            "tool_call",
            "recall",
            "error",
            Some(20),
            Some(&json!({ "error": "boom" })),
        )
        .unwrap();
        record(&c, 103, "tool_call", "graph_stats", "ok", Some(5), None).unwrap();

        let snap = snapshot(&c, 10).unwrap();
        assert_eq!(snap.total_events, 4);
        assert_eq!(snap.total_errors, 1);

        let recall = snap.by_name.iter().find(|m| m.name == "recall").unwrap();
        assert_eq!(recall.calls, 3);
        assert_eq!(recall.errors, 1);
        assert_eq!(recall.total_ms, 60);
        assert_eq!(recall.min_ms, 10);
        assert_eq!(recall.max_ms, 30);
        assert_eq!(recall.avg_ms, 20.0);

        // by_name is ordered by call count desc, so the busiest tool leads.
        assert_eq!(snap.by_name[0].name, "recall");
    }

    #[test]
    fn snapshot_recent_is_newest_first_and_bounded() {
        let c = conn();
        for i in 0..5 {
            record(&c, 200 + i, "tool_call", "ingest_file", "ok", Some(i), None).unwrap();
        }
        let snap = snapshot(&c, 2).unwrap();
        assert_eq!(snap.recent.len(), 2);
        // Most-recent first: the last inserted event leads.
        assert_eq!(snap.recent[0].ts, 204);
        assert_eq!(snap.recent[1].ts, 203);
    }

    #[test]
    fn record_roundtrips_detail_json() {
        let c = conn();
        record(
            &c,
            1,
            "tool_call",
            "get_impact_radius",
            "ok",
            Some(7),
            Some(&json!({ "nodes": 3 })),
        )
        .unwrap();
        let snap = snapshot(&c, 1).unwrap();
        let detail = snap.recent[0].detail.as_ref().unwrap();
        assert_eq!(detail["nodes"], 3);
    }
}

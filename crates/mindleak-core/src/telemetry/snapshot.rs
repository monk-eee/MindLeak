//! Aggregate metrics plus the most recent events: the main read path a
//! caller uses to build a point-in-time observability view.

use std::collections::HashMap;

use rusqlite::Connection;
use serde_json::Value;

use super::habits::memory_habits;
use super::record::ensure_table;
use super::retrospective::usage_retrospective;
use super::types::{EventRow, NameMetric, Snapshot};
use crate::error::Result;

/// Aggregate metrics plus the most recent `recent_limit` events.
pub fn snapshot(conn: &Connection, recent_limit: usize) -> Result<Snapshot> {
    ensure_table(conn)?;

    // The `detail` of each tool's most recent error, keyed by name. Identified
    // by the largest `id` among that tool's error rows so it survives even after
    // the raw event scrolls out of the bounded `recent` window below.
    let mut last_error_detail: HashMap<String, Option<Value>> = HashMap::new();
    let mut last_degraded_detail: HashMap<String, Option<Value>> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT t.name, t.outcome, t.detail
             FROM telemetry_events t
             JOIN (
                 SELECT name, outcome, MAX(id) AS latest_id
                 FROM telemetry_events
                 WHERE outcome IN ('error', 'skipped')
                 GROUP BY name, outcome
             ) latest ON t.id = latest.latest_id",
        )?;
        let rows = stmt.query_map([], |row| {
            let name: String = row.get(0)?;
            let outcome: String = row.get(1)?;
            let detail: Option<String> = row.get(2)?;
            Ok((name, outcome, detail))
        })?;
        for row in rows {
            let (name, outcome, detail) = row?;
            let detail = detail.and_then(|value| serde_json::from_str(&value).ok());
            if outcome == "error" {
                last_error_detail.insert(name, detail);
            } else {
                last_degraded_detail.insert(name, detail);
            }
        }
    }

    let mut by_name = Vec::new();
    {
        let mut stmt = conn.prepare(
            "WITH latest_outcomes AS (
                 SELECT event.name, event.outcome
                 FROM telemetry_events event
                 JOIN (
                     SELECT name, MAX(id) AS latest_id
                     FROM telemetry_events
                     GROUP BY name
                 ) latest ON event.id = latest.latest_id
             )
             SELECT event.name,
                    COUNT(*)                                           AS calls,
                    SUM(CASE WHEN event.outcome = 'error' THEN 1 ELSE 0 END) AS errors,
                    COALESCE(SUM(event.duration_ms), 0)                AS total_ms,
                    COALESCE(MIN(event.duration_ms), 0)                AS min_ms,
                    COALESCE(MAX(event.duration_ms), 0)                AS max_ms,
                    MAX(CASE WHEN event.outcome = 'ok' THEN event.ts END) AS last_success_at,
                    MAX(CASE WHEN event.outcome = 'error' THEN event.ts END) AS last_error_at,
                    MAX(CASE WHEN event.outcome = 'skipped' THEN event.ts END) AS last_degraded_at,
                    latest_outcomes.outcome                            AS latest_outcome
             FROM telemetry_events event
             JOIN latest_outcomes ON latest_outcomes.name = event.name
             GROUP BY event.name, latest_outcomes.outcome
             ORDER BY calls DESC, event.name ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let calls: i64 = row.get(1)?;
            let total_ms: i64 = row.get(3)?;
            let name: String = row.get(0)?;
            let last_success_at: Option<i64> = row.get(6)?;
            let last_error_at: Option<i64> = row.get(7)?;
            let last_degraded_at: Option<i64> = row.get(8)?;
            let latest_outcome: String = row.get(9)?;
            Ok(NameMetric {
                name,
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
                last_success_at,
                last_error_at,
                last_error_detail: None,
                last_degraded_at,
                last_degraded_detail: None,
                currently_failing: latest_outcome == "error",
                currently_degraded: latest_outcome == "skipped",
            })
        })?;
        for row in rows {
            by_name.push(row?);
        }
    }
    for metric in &mut by_name {
        if let Some(detail) = last_error_detail.remove(&metric.name) {
            metric.last_error_detail = detail;
        }
        if let Some(detail) = last_degraded_detail.remove(&metric.name) {
            metric.last_degraded_detail = detail;
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

    let memory_habits = memory_habits(conn)?;
    let retrospective = usage_retrospective(&by_name, &memory_habits, crate::now_unix());
    Ok(Snapshot {
        total_events,
        total_errors,
        currently_failing_tools: by_name.iter().filter(|m| m.currently_failing).count() as i64,
        currently_degraded_tools: by_name
            .iter()
            .filter(|metric| metric.currently_degraded)
            .count() as i64,
        by_name,
        recent,
        memory_habits,
        retrospective,
    })
}

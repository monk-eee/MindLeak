//! Table lifecycle and the append-only write path.

use rusqlite::{params, Connection};
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

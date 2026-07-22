//! SQLite connection setup, migrations, and the `effective_weight` SQL function.

use rusqlite::functions::FunctionFlags;
use rusqlite::Connection;

use crate::error::Result;

const SCHEMA: &str = include_str!("schema.sql");

/// Open (or create) a MindLeak database at `path`, apply the schema, and register
/// the `effective_weight` scalar SQL function used by graph queries.
pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    Ok(conn)
}

/// Open an in-memory database (used by tests and ephemeral tooling).
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    conn.execute_batch(SCHEMA)?;
    migrate(conn)?;
    register_functions(conn)?;
    Ok(())
}

/// Register `effective_weight(base, half_life_hours, updated_at, now)` so the
/// decay formula can run inside SQL (filtering/ordering) without SQLite math
/// extensions.
fn register_functions(conn: &Connection) -> Result<()> {
    conn.create_scalar_function(
        "effective_weight",
        4,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let base: f64 = ctx.get(0)?;
            let half_life: f64 = ctx.get(1)?;
            let updated_at: i64 = ctx.get(2)?;
            let now: i64 = ctx.get(3)?;
            Ok(crate::decay::effective_weight(
                base, half_life, updated_at, now,
            ))
        },
    )?;
    conn.create_scalar_function(
        "signal_half_life",
        4,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let base: f64 = ctx.get(0)?;
            let count: i64 = ctx.get(1)?;
            let first_seen: i64 = ctx.get(2)?;
            let updated_at: i64 = ctx.get(3)?;
            Ok(crate::decay::signal_half_life(
                base, count, first_seen, updated_at,
            ))
        },
    )?;
    Ok(())
}

/// Idempotently add the signal-weighted-decay columns to a pre-existing `edges`
/// table (fresh databases already have them from the schema). Backfills
/// `first_seen` to `updated_at` so migrated edges start with a zero span and earn
/// no signal bonus until genuinely reinforced over time.
fn migrate(conn: &Connection) -> Result<()> {
    if !column_exists(conn, "edges", "first_seen")? {
        conn.execute(
            "ALTER TABLE edges ADD COLUMN first_seen INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
        conn.execute(
            "UPDATE edges SET first_seen = updated_at WHERE first_seen = 0",
            [],
        )?;
    }
    if !column_exists(conn, "edges", "reinforcement_count")? {
        conn.execute(
            "ALTER TABLE edges ADD COLUMN reinforcement_count INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

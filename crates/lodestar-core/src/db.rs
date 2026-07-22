//! SQLite connection setup, schema application, and the `effective_weight`
//! scalar function used for knowledge revalidation queries.

use rusqlite::functions::FunctionFlags;
use rusqlite::Connection;

use crate::error::Result;

const SCHEMA: &str = include_str!("schema.sql");

/// Open (or create) a Lodestar database at `path` and configure it.
pub fn open(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    configure(&conn)?;
    Ok(conn)
}

/// Open an in-memory database (tests / ephemeral tooling).
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    // WAL + a generous busy timeout: many local agents/worktrees share one file
    // and race on the claim CAS; SQLite serialises writers, the timeout absorbs
    // contention instead of erroring.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    conn.execute_batch(SCHEMA)?;
    migrate(conn)?;
    register_functions(conn)?;
    Ok(())
}

fn migrate(conn: &Connection) -> Result<()> {
    for (table, column, definition) in [
        ("tasks", "claim_started_at", "INTEGER"),
        ("goal_code", "mode", "TEXT NOT NULL DEFAULT 'governed'"),
        ("conformance", "evidence_schema_version", "INTEGER"),
        ("conformance", "evidence", "TEXT"),
    ] {
        if !column_exists(conn, table, column)? {
            conn.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition}"
            ))?;
        }
    }
    conn.execute(
        "UPDATE tasks
         SET claim_started_at = updated_at
         WHERE status = 'claimed' AND claim_started_at IS NULL",
        [],
    )?;
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn register_functions(conn: &Connection) -> Result<()> {
    conn.create_scalar_function(
        "effective_weight",
        4,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let base: f64 = ctx.get(0)?;
            let half_life: f64 = ctx.get(1)?;
            let confirmed_at: i64 = ctx.get(2)?;
            let now: i64 = ctx.get(3)?;
            Ok(crate::decay::effective_weight(
                base,
                half_life,
                confirmed_at,
                now,
            ))
        },
    )?;
    Ok(())
}

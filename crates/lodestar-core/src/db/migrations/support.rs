//! The plumbing a migration runs on: how one is recorded as applied, and what
//! it may ask about the shape it is migrating.
//!
//! Separate from the migrations themselves so `migrations.rs` reads as the list
//! of what changed, not the mechanism that changes it.

use rusqlite::{Connection, OptionalExtension};

use crate::error::Result;

/// Run a migration at most once per database, recorded by name (ADR-0063).
///
/// Pattern-idempotence — "rewrite every row that still looks unmigrated" — is
/// only idempotent while nothing else creates such rows. A rewrite that races a
/// live writer re-fires on every open, forever, and each firing looks exactly
/// like the first. That is not a theoretical hazard: it re-owned a live claim
/// out from under its holder every time any process opened the database, which
/// is how a task ended up provable by nobody. Anything touching identity or
/// ownership belongs here rather than in a pattern-guarded loop.
pub(super) fn run_once(
    connection: &Connection,
    name: &str,
    migration: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let applied: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM schema_migrations WHERE name = ?1",
            [name],
            |row| row.get(0),
        )
        .optional()?;
    if applied.is_some() {
        return Ok(());
    }
    migration()?;
    connection.execute(
        "INSERT INTO schema_migrations (name, applied_at) VALUES (?1, ?2)",
        rusqlite::params![name, crate::now_unix()],
    )?;
    Ok(())
}

pub(crate) fn column_exists(connection: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn table_exists(connection: &Connection, table: &str) -> Result<bool> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get::<_, bool>(0),
    )?)
}

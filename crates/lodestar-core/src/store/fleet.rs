//! Durable storage for declared session context (ADR-0035, ADR-0043).

use mindleak_session::SessionContext;
use rusqlite::{params, OptionalExtension, Row};

use crate::error::Result;

use super::{collect, LodestarStore};

const CONTEXT_COLS: &str = "agent_id, branch, head_sha, base, dirty, behind, declared_at";

impl LodestarStore {
    /// Record where a session says it is working, replacing any earlier
    /// declaration for that agent.
    ///
    /// Replace rather than merge: the client is the source of truth for its own
    /// position, so a later declaration that omits a field is asserting that the
    /// field is no longer known — not asking to keep a stale value.
    pub fn declare_session_context(
        &self,
        agent_id: &str,
        context: &SessionContext,
        now: i64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO session_context (agent_id, branch, head_sha, base, dirty, behind, declared_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(agent_id) DO UPDATE SET
                 branch = excluded.branch,
                 head_sha = excluded.head_sha,
                 base = excluded.base,
                 dirty = excluded.dirty,
                 behind = excluded.behind,
                 declared_at = excluded.declared_at",
            params![
                agent_id,
                context.branch,
                context.head_sha,
                context.base,
                context.dirty,
                context.behind,
                now,
            ],
        )?;
        Ok(())
    }

    /// One agent's declared context and when it was declared.
    pub fn session_context(&self, agent_id: &str) -> Result<Option<(SessionContext, i64)>> {
        let sql = format!("SELECT {CONTEXT_COLS} FROM session_context WHERE agent_id = ?1");
        Ok(self
            .conn
            .query_row(&sql, params![agent_id], row_to_context)
            .optional()?
            .map(|(_, context, declared_at)| (context, declared_at)))
    }

    /// Every declared context, oldest declaration first.
    pub fn declared_contexts(&self) -> Result<Vec<(String, SessionContext, i64)>> {
        let sql = format!(
            "SELECT {CONTEXT_COLS} FROM session_context ORDER BY declared_at ASC, agent_id ASC"
        );
        let mut statement = self.conn.prepare(&sql)?;
        let rows = statement.query_map([], row_to_context)?;
        collect(rows)
    }
}

fn row_to_context(row: &Row<'_>) -> rusqlite::Result<(String, SessionContext, i64)> {
    Ok((
        row.get(0)?,
        SessionContext {
            branch: row.get(1)?,
            head_sha: row.get(2)?,
            base: row.get(3)?,
            dirty: row.get(4)?,
            behind: row.get(5)?,
        },
        row.get(6)?,
    ))
}

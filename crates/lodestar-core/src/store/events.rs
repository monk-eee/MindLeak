//! The append-only task lifecycle log (ADR-0064).
//!
//! Every transition the executive performs appends one row here, in the same
//! transaction as the guarded write it records. `tasks` is the projection: the
//! row and the record are committed together, so the projection is derivable
//! from the log rather than merely consistent with it by habit.
//!
//! Nothing in this module reads a clock. `recorded_at` is supplied by the
//! caller that already has the `now` it used for the transition, so replaying
//! the log is a deterministic assignment and a rebuild can be diffed against
//! the live table.

use rusqlite::{params, Connection};

use super::LodestarStore;
use crate::error::{LodestarError, Result};
use crate::model::{TaskEvent, TaskEventKind};

const EVENT_COLS: &str = "seq, task_id, kind, actor, recorded_at, after, detail";

/// Append one transition to the log, returning its sequence number.
///
/// Takes a `&Connection` rather than `&self` so callers can pass the
/// [`rusqlite::Transaction`] that is already open for the state write. That is
/// the whole point: an event committed in a different transaction from the row
/// it describes could be present without the row, or the row without the event,
/// and the projection would stop being checkable.
///
/// `task_id` is taken from `after.id` rather than accepted separately, so an
/// event cannot claim to describe one task while carrying the after-image of
/// another.
pub(crate) fn append(
    connection: &Connection,
    kind: TaskEventKind,
    actor: Option<&str>,
    recorded_at: i64,
    after: &crate::model::Task,
    detail: &str,
) -> Result<i64> {
    connection.execute(
        "INSERT INTO task_events (task_id, kind, actor, recorded_at, after, detail)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            after.id,
            kind.as_str(),
            actor,
            recorded_at,
            serde_json::to_string(after)?,
            detail
        ],
    )?;
    Ok(connection.last_insert_rowid())
}

/// Read events in application order, optionally narrowed to one task.
fn read(connection: &Connection, task_id: Option<&str>) -> Result<Vec<TaskEvent>> {
    let sql = match task_id {
        Some(_) => format!("SELECT {EVENT_COLS} FROM task_events WHERE task_id = ?1 ORDER BY seq"),
        None => format!("SELECT {EVENT_COLS} FROM task_events ORDER BY seq"),
    };
    let mut statement = connection.prepare(&sql)?;
    let mut rows = match task_id {
        Some(id) => statement.query(params![id])?,
        None => statement.query([])?,
    };

    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let kind: String = row.get(2)?;
        let after: String = row.get(5)?;
        out.push(TaskEvent {
            seq: row.get(0)?,
            task_id: row.get(1)?,
            kind: TaskEventKind::from_tag(&kind).ok_or_else(|| {
                LodestarError::Invalid(format!("unknown task event kind: {kind}"))
            })?,
            actor: row.get(3)?,
            recorded_at: row.get(4)?,
            after: serde_json::from_str(&after)?,
            detail: row.get(6)?,
        });
    }
    Ok(out)
}

impl LodestarStore {
    /// Every recorded transition for one task, oldest first.
    pub fn task_events(&self, task_id: &str) -> Result<Vec<TaskEvent>> {
        read(&self.conn, Some(task_id))
    }

    /// The whole task log in application order.
    pub fn task_log(&self) -> Result<Vec<TaskEvent>> {
        read(&self.conn, None)
    }
}

/// Seed the log with one genesis event per task that predates it (ADR-0064).
///
/// A genesis event carries the task as it currently stands and **no history
/// before it**. The claims, lapses and transitions that produced that row were
/// never recorded and cannot be reconstructed; synthesising plausible ones
/// would place invented history inside an audit ledger, which is precisely the
/// failure this ledger exists to refuse. The absence of events before a genesis
/// is a fact about this database, not a gap to be filled.
///
/// This never writes to `tasks`. ADR-0063 is explicit that a live claim is not
/// ours to rewrite, and appending a record of the present transfers nothing:
/// after this runs, every claim is held by exactly whoever held it before.
///
/// Tasks that already have events are skipped, so a database that has been
/// running with the log does not acquire a second genesis. The caller is
/// nonetheless expected to wrap this in `run_once` (ADR-0063 decision 3):
/// pattern-idempotence is not idempotence when a live writer can recreate the
/// pattern.
pub(crate) fn import_genesis(connection: &Connection, now: i64) -> Result<usize> {
    let sql = format!(
        "SELECT {} FROM {} t WHERE NOT EXISTS \
         (SELECT 1 FROM task_events e WHERE e.task_id = t.id) ORDER BY t.created_at, t.id",
        super::coordination::TASK_COLS
            .split(", ")
            .map(|c| format!("t.{c}"))
            .collect::<Vec<_>>()
            .join(", "),
        "tasks"
    );
    let mut statement = connection.prepare(&sql)?;
    let tasks = statement
        .query_map([], super::coordination::row_to_task)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);

    for task in &tasks {
        append(connection, TaskEventKind::Imported, None, now, task, "")?;
    }
    Ok(tasks.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TaskStatus;
    use crate::store::test_support::{goal, store, NOW};

    fn seeded(s: &LodestarStore) -> crate::model::Task {
        let g = goal(s);
        s.create_task(&g.id, "a task", "acceptance", None, NOW)
            .unwrap()
    }

    #[test]
    fn an_appended_event_reads_back_with_its_after_image_intact() {
        let s = store();
        let task = seeded(&s);

        append(
            &s.conn,
            TaskEventKind::Claimed,
            Some("session:v1:abc"),
            1_700_000_000,
            &task,
            r#"{"lease_secs":300}"#,
        )
        .unwrap();

        let events = s.task_events(&task.id).unwrap();
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.kind, TaskEventKind::Claimed);
        assert_eq!(event.actor.as_deref(), Some("session:v1:abc"));
        assert_eq!(event.recorded_at, 1_700_000_000);
        assert_eq!(event.task_id, task.id);
        assert_eq!(event.after.id, task.id);
        assert_eq!(event.after.title, task.title);
        assert_eq!(event.after.status, TaskStatus::Open);
        assert_eq!(event.detail, r#"{"lease_secs":300}"#);
    }

    #[test]
    fn events_read_back_in_application_order_not_insertion_luck() {
        let s = store();
        let task = seeded(&s);

        for (kind, at) in [
            (TaskEventKind::Created, 10),
            (TaskEventKind::Claimed, 20),
            (TaskEventKind::Released, 30),
        ] {
            append(&s.conn, kind, None, at, &task, "").unwrap();
        }

        let kinds: Vec<_> = s
            .task_events(&task.id)
            .unwrap()
            .iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                TaskEventKind::Created,
                TaskEventKind::Claimed,
                TaskEventKind::Released
            ]
        );

        let seqs: Vec<_> = s
            .task_events(&task.id)
            .unwrap()
            .iter()
            .map(|e| e.seq)
            .collect();
        let mut sorted = seqs.clone();
        sorted.sort_unstable();
        assert_eq!(seqs, sorted, "seq must be the total order of application");
    }

    #[test]
    fn the_task_log_spans_every_task_while_a_task_read_is_narrowed() {
        let s = store();
        let one = seeded(&s);
        let two = seeded(&s);

        append(&s.conn, TaskEventKind::Created, None, 1, &one, "").unwrap();
        append(&s.conn, TaskEventKind::Created, None, 2, &two, "").unwrap();
        append(&s.conn, TaskEventKind::Claimed, None, 3, &two, "").unwrap();

        assert_eq!(s.task_log().unwrap().len(), 3);
        assert_eq!(s.task_events(&one.id).unwrap().len(), 1);
        assert_eq!(s.task_events(&two.id).unwrap().len(), 2);
    }

    /// A record of what happened must outlive its subject. `task_claim_transfers`
    /// cascades on delete, which is right for an audit of a row that must exist;
    /// applying that here would let deleting a task erase the evidence that it
    /// ever existed, which is the opposite of the point.
    #[test]
    fn deleting_the_task_does_not_delete_its_history() {
        let s = store();
        let task = seeded(&s);
        append(&s.conn, TaskEventKind::Created, None, 1, &task, "").unwrap();

        s.conn
            .execute("DELETE FROM tasks WHERE id = ?1", params![task.id])
            .unwrap();

        assert_eq!(
            s.task_events(&task.id).unwrap().len(),
            1,
            "the log is not a child of the row it describes"
        );
    }

    #[test]
    fn an_unrecognised_kind_is_refused_rather_than_silently_coerced() {
        let s = store();
        let task = seeded(&s);
        s.conn
            .execute(
                "INSERT INTO task_events (task_id, kind, actor, recorded_at, after, detail)
                 VALUES (?1, 'teleported', NULL, 1, ?2, '')",
                params![task.id, serde_json::to_string(&task).unwrap()],
            )
            .unwrap();

        let err = s.task_events(&task.id).unwrap_err();
        assert!(
            matches!(err, LodestarError::Invalid(ref m) if m.contains("teleported")),
            "expected the unknown kind to be named, got: {err}"
        );
    }
}

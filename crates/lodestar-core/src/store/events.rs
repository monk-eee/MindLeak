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

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeMap;

use super::LodestarStore;
use crate::error::{LodestarError, Result};
use crate::model::{ClaimWindow, TaskEvent, TaskEventKind};

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

    /// Rebuild task state purely from the log, ordered by id.
    ///
    /// This is ADR-0064 decision 4: the check that keeps decision 2 honest.
    /// `tasks` is written through rather than rebuilt, because ADR-0063 forbids
    /// a migration from touching a live claim — which means "the projection is
    /// derivable from the log" is a claim that could quietly stop being true.
    /// So it is asserted rather than assumed: if this disagrees with `tasks`, a
    /// transition was applied without being recorded, and the log has a hole in
    /// it that nothing else would report.
    ///
    /// Each event carries the after-image its transition produced, so replay is
    /// an assignment in `seq` order and the last event per task wins.
    pub fn project_tasks(&self) -> Result<Vec<crate::model::Task>> {
        let mut projected: BTreeMap<String, crate::model::Task> = BTreeMap::new();
        for event in self.task_log()? {
            projected.insert(event.task_id.clone(), event.after);
        }
        Ok(projected.into_values().collect())
    }
}

/// Record a transition that has **just been applied**, inside the same
/// transaction that applied it.
///
/// Reads the task back so the event carries the state the transition actually
/// produced rather than what the caller believed it would produce — the guarded
/// UPDATE does conditional arithmetic (see the claim CAS), and a caller
/// reconstructing the outcome by hand would be a second implementation of it.
///
/// Call this only after the guard reported it changed a row. An event for a
/// transition that did not happen is worse than no log at all: it is a receipt
/// for work nobody did.
pub(super) fn record(
    connection: &Connection,
    id: &str,
    kind: TaskEventKind,
    actor: Option<&str>,
    now: i64,
    detail: &str,
) -> Result<()> {
    let task = super::coordination::get_task_on(connection, id)?
        .ok_or_else(|| LodestarError::NotFound(id.to_string()))?;
    append(connection, kind, actor, now, &task, detail)?;
    Ok(())
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
        // The genesis carries the counters it found, because they are the only
        // surviving trace of a window that opened before the log did (ADR-0064
        // decision 6). Deriving continuity purely from in-log transitions would
        // report zero lapses for such a window, and under ADR-0048 a window
        // with no lapses may certify itself as aligned — so dropping them here
        // would launder a discontinuous window clean during a migration.
        //
        // Read straight off the row rather than through `Task`, which no longer
        // has these fields (ADR-0064 d5). A database created after the columns
        // were dropped has no holes to carry, and seeds from nothing.
        let seed = legacy_counters(connection, &task.id)?;
        append(
            connection,
            TaskEventKind::Imported,
            None,
            now,
            task,
            &match seed {
                Some((lapses, unleased)) => serde_json::json!({
                    "claim_lapses": lapses,
                    "unleased_seconds": unleased,
                })
                .to_string(),
                None => String::new(),
            },
        )?;
    }
    Ok(tasks.len())
}

/// The pre-ADR-0064 running totals for one task, when the columns are still
/// present. `None` once they have been dropped, or on a database created after
/// they were: there is then no pre-log window to carry forward.
fn legacy_counters(connection: &Connection, task_id: &str) -> Result<Option<(i64, i64)>> {
    if !crate::db::column_exists(connection, "tasks", "claim_lapses")? {
        return Ok(None);
    }
    Ok(connection
        .query_row(
            "SELECT claim_lapses, unleased_seconds FROM tasks WHERE id = ?1",
            params![task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}

/// The continuity of a task's current evidence window, derived from the log
/// (ADR-0064 decision 6).
///
/// Replays the recorded transitions rather than reading a running total off the
/// task row. A window is identified by its owner and the instant it opened; the
/// same owner re-claiming keeps both, which is what makes a lapse detectable as
/// "a claim whose predecessor's lease had already expired".
///
/// Where the window opened before the log existed, the genesis event carries
/// the counters that were current at import and this seeds from them. A window
/// that opened *after* the genesis starts clean, because its whole history is
/// present and can be trusted to be complete.
pub(crate) fn claim_window_on(connection: &Connection, task_id: &str) -> Result<ClaimWindow> {
    let events = read(connection, Some(task_id))?;

    let mut window: Option<(Option<String>, Option<i64>)> = None;
    let mut lapses = 0i64;
    let mut unleased = 0i64;
    let mut previous_lease: Option<i64> = None;

    for event in &events {
        let key = (event.after.owner.clone(), event.after.claim_started_at);
        let same_window = window.as_ref() == Some(&key);

        if event.kind == TaskEventKind::Imported {
            let seed: serde_json::Value =
                serde_json::from_str(&event.detail).unwrap_or(serde_json::Value::Null);
            lapses = seed
                .get("claim_lapses")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            unleased = seed
                .get("unleased_seconds")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
        } else if !same_window {
            // A different owner, or the same owner opening a fresh window:
            // the previous window's holes do not travel with it.
            lapses = 0;
            unleased = 0;
        } else if event.kind == TaskEventKind::Claimed {
            // A re-claim inside a window it did not open means the lease had
            // run out first. That gap is the hole ADR-0048 counts.
            if let Some(expired_at) = previous_lease {
                if expired_at < event.recorded_at {
                    lapses += 1;
                    unleased += event.recorded_at - expired_at;
                }
            }
        }

        window = Some(key);
        previous_lease = event.after.lease_expires_at;
    }

    Ok(ClaimWindow {
        started_at: window.and_then(|(_, started)| started),
        lapses,
        unleased_seconds: unleased,
    })
}

impl LodestarStore {
    /// The continuity of a task's current evidence window (ADR-0064 d6).
    pub fn claim_window(&self, task_id: &str) -> Result<ClaimWindow> {
        claim_window_on(&self.conn, task_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::TaskStatus;
    use crate::store::test_support::{complete_aligned, goal, store, NOW};

    fn seeded(s: &LodestarStore) -> crate::model::Task {
        seeded_named(s, "a task")
    }

    /// Distinct titles give distinct ids: a task id hashes goal|title|created_at,
    /// so two tasks seeded with the same title at the same instant would collide
    /// and their logs would silently merge into one.
    fn seeded_named(s: &LodestarStore, title: &str) -> crate::model::Task {
        let g = goal(s);
        s.create_task(&g.id, title, "acceptance", None, NOW)
            .unwrap()
    }

    fn kinds_of(s: &LodestarStore, task_id: &str) -> Vec<TaskEventKind> {
        s.task_events(task_id)
            .unwrap()
            .into_iter()
            .map(|e| e.kind)
            .collect()
    }

    /// Creating a task is itself a transition, so a freshly seeded task already
    /// carries exactly one event. Every other test in this module asserts
    /// against what it appended rather than against a total, so this birth
    /// certificate does not have to be counted into each expectation.
    #[test]
    fn creating_a_task_is_itself_recorded() {
        let s = store();
        let task = seeded(&s);

        let events = s.task_events(&task.id).unwrap();
        assert_eq!(events.len(), 1, "a task's own creation is a transition");
        assert_eq!(events[0].kind, TaskEventKind::Created);
        assert!(
            events[0].actor.is_none(),
            "creation is requested, not performed by a claim holder"
        );
        assert_eq!(events[0].after.id, task.id);
        assert_eq!(events[0].after.status, TaskStatus::Open);
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
        let event = events.last().expect("the appended event");
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
            (TaskEventKind::Claimed, 20),
            (TaskEventKind::Paused, 25),
            (TaskEventKind::Released, 30),
        ] {
            append(&s.conn, kind, None, at, &task, "").unwrap();
        }

        assert_eq!(
            kinds_of(&s, &task.id),
            vec![
                TaskEventKind::Created,
                TaskEventKind::Claimed,
                TaskEventKind::Paused,
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
        let one = seeded_named(&s, "the first task");
        let two = seeded_named(&s, "the second task");
        assert_ne!(one.id, two.id);

        append(&s.conn, TaskEventKind::Claimed, None, 3, &two, "").unwrap();

        assert_eq!(s.task_log().unwrap().len(), 3, "two creations and a claim");
        assert_eq!(kinds_of(&s, &one.id), vec![TaskEventKind::Created]);
        assert_eq!(
            kinds_of(&s, &two.id),
            vec![TaskEventKind::Created, TaskEventKind::Claimed]
        );
    }

    /// A record of what happened must outlive its subject. `task_claim_transfers`
    /// cascades on delete, which is right for an audit of a row that must exist;
    /// applying that here would let deleting a task erase the evidence that it
    /// ever existed, which is the opposite of the point.
    #[test]
    fn deleting_the_task_does_not_delete_its_history() {
        let s = store();
        let task = seeded(&s);
        let before = s.task_events(&task.id).unwrap().len();
        assert!(before > 0);

        s.conn
            .execute("DELETE FROM tasks WHERE id = ?1", params![task.id])
            .unwrap();

        assert_eq!(
            s.task_events(&task.id).unwrap().len(),
            before,
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

    /// ADR-0064 decision 4. `tasks` is written through rather than rebuilt, so
    /// "the projection is derivable from the log" is a property that could
    /// quietly stop holding the moment a verb forgets to record itself. This
    /// walks a task through most of its lifecycle and asserts the log alone
    /// reproduces the live table — a verb that mutates state without appending
    /// an event fails here and nowhere else.
    #[test]
    fn replaying_the_log_reproduces_the_live_board_exactly() {
        let s = store();
        let g = goal(&s);

        // A claimed task that lapsed and was re-claimed by its owner.
        let lapsed = s
            .create_task(&g.id, "lapsed then reclaimed", "", None, NOW)
            .unwrap();
        assert!(s.claim_task(&lapsed.id, "alice", 60, NOW).unwrap());
        assert!(s.claim_task(&lapsed.id, "alice", 60, NOW + 500).unwrap());
        assert!(s.renew_lease(&lapsed.id, "alice", 90, NOW + 510).unwrap());

        // Claimed, parked with a question, answered, paused, resumed, released.
        let parked = s
            .create_task(&g.id, "asked and answered", "", None, NOW)
            .unwrap();
        assert!(s.claim_task(&parked.id, "bob", 60, NOW).unwrap());
        assert!(s
            .ask_question(&parked.id, "bob", "which way?", None, NOW + 1)
            .unwrap());
        assert!(s
            .answer_question(&parked.id, "that way", "human", 60, NOW + 2)
            .unwrap());
        assert!(s
            .pause_task(&parked.id, "bob", Some("lunch"), NOW + 3)
            .unwrap());
        assert!(s.resume_task(&parked.id, "bob", 60, NOW + 4).unwrap());
        assert!(s.release_task(&parked.id, "bob", NOW + 5).unwrap());

        // Blocked, then reopened.
        let blocked = s
            .create_task(&g.id, "blocked then reopened", "", None, NOW)
            .unwrap();
        assert!(s
            .block_task(
                &blocked.id,
                None,
                Some("waiting on a decision"),
                "carol",
                NOW + 6
            )
            .unwrap());
        assert!(s.reopen_task(&blocked.id, NOW + 7).unwrap());

        // Completed through conformance, and abandoned.
        let done = s.create_task(&g.id, "completed", "", None, NOW).unwrap();
        complete_aligned(&s, &done.id, "dave", NOW + 8);

        let dropped = s.create_task(&g.id, "abandoned", "", None, NOW).unwrap();
        assert!(s.abandon_task(&dropped.id, NOW + 9).unwrap());

        let mut live = s.board(true).unwrap();
        live.sort_by(|a, b| a.id.cmp(&b.id));
        let projected = s.project_tasks().unwrap();

        assert_eq!(live.len(), 5, "every task should be on the board");
        assert_eq!(
            projected.len(),
            live.len(),
            "the log knows about a different number of tasks than the board"
        );
        for (live, projected) in live.iter().zip(projected.iter()) {
            assert_eq!(
                serde_json::to_value(live).unwrap(),
                serde_json::to_value(projected).unwrap(),
                "task {} differs between the live row and its replay; \
                 a transition was applied without being recorded",
                live.id
            );
        }
    }

    /// A predecessor completing moves its successor without any agent asking,
    /// so that transition has no actor — but it is still a transition, and a
    /// log that missed it would show a task springing from `blocked` to `open`
    /// with nothing to explain it.
    #[test]
    fn a_successor_unblocked_by_its_predecessor_is_recorded_with_no_actor() {
        let s = store();
        let g = goal(&s);
        let first = s.create_task(&g.id, "first", "", None, NOW).unwrap();
        let second = s
            .create_task_after(&g.id, "second", "", None, Some(first.id.clone()), NOW)
            .unwrap();

        complete_aligned(&s, &first.id, "alice", NOW + 1);

        let reopened: Vec<_> = s
            .task_events(&second.id)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == TaskEventKind::Reopened)
            .collect();
        assert_eq!(reopened.len(), 1, "the unblocking should be recorded once");
        assert!(
            reopened[0].actor.is_none(),
            "nobody asked for this; naming an actor would attribute a decision never made"
        );
        assert!(reopened[0].detail.contains(&first.id));
        assert_eq!(reopened[0].after.status, TaskStatus::Open);
    }

    /// The claim CAS keeps the evidence window open across a lapse (ADR-0048).
    /// The event carries the outcome the guarded UPDATE actually produced, so
    /// this pins that the recorded window start matches what landed in the row.
    #[test]
    fn a_claim_event_carries_the_state_the_guarded_update_actually_produced() {
        let s = store();
        let g = goal(&s);
        let task = s.create_task(&g.id, "lapse me", "", None, NOW).unwrap();

        assert!(s.claim_task(&task.id, "alice", 60, NOW).unwrap());
        assert!(s.claim_task(&task.id, "alice", 60, NOW + 500).unwrap());

        let claims: Vec<_> = s
            .task_events(&task.id)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == TaskEventKind::Claimed)
            .collect();
        assert_eq!(claims.len(), 2);
        assert_eq!(
            claims[0].after.claim_started_at, claims[1].after.claim_started_at,
            "the evidence window survives the lapse (ADR-0048)"
        );
        assert_eq!(
            claims[0].after.lease_expires_at,
            Some(NOW + 60),
            "the first lease is the one the second claim found expired"
        );

        let live = s.get_task(&task.id).unwrap().unwrap();
        assert_eq!(claims[1].after.lease_expires_at, live.lease_expires_at);
        assert_eq!(claims[1].after.claim_started_at, live.claim_started_at);
    }

    #[test]
    fn a_losing_claim_records_nothing() {
        let s = store();
        let g = goal(&s);
        let task = s.create_task(&g.id, "contested", "", None, NOW).unwrap();

        assert!(s.claim_task(&task.id, "alice", 600, NOW).unwrap());
        assert!(!s.claim_task(&task.id, "bob", 600, NOW + 1).unwrap());

        let claims = s
            .task_events(&task.id)
            .unwrap()
            .into_iter()
            .filter(|e| e.kind == TaskEventKind::Claimed)
            .count();
        assert_eq!(
            claims, 1,
            "an event for a transition that did not happen is a receipt for work nobody did"
        );
    }

    /// The window start is the one thing continuity still shares with the task
    /// row, so it stays checkable against it. The lapse counts no longer have a
    /// column to agree with — that equality was proved in the commit that
    /// introduced the derivation, while both still existed.
    fn assert_window_start_matches_row(s: &LodestarStore, task_id: &str, note: &str) {
        let live = s.get_task(task_id).unwrap().unwrap();
        let derived = s.claim_window(task_id).unwrap();
        assert_eq!(
            derived.started_at, live.claim_started_at,
            "{note}: derived window start disagrees with the row"
        );
    }

    #[test]
    fn a_window_never_claimed_is_continuous_and_has_not_started() {
        let s = store();
        let task = seeded(&s);
        let window = s.claim_window(&task.id).unwrap();
        assert!(window.is_continuous());
        assert_eq!(window.started_at, None);
        assert_window_start_matches_row(&s, &task.id, "never claimed");
    }

    #[test]
    fn a_clean_claim_and_its_renewals_leave_the_window_continuous() {
        let s = store();
        let g = goal(&s);
        let t = s.create_task(&g.id, "clean", "", None, NOW).unwrap();

        assert!(s.claim_task(&t.id, "alice", 600, NOW).unwrap());
        assert!(s.renew_lease(&t.id, "alice", 600, NOW + 10).unwrap());
        assert!(s.touch_lease(&t.id, "alice", 600, NOW + 20).unwrap());

        let window = s.claim_window(&t.id).unwrap();
        assert!(window.is_continuous(), "renewal is not a lapse");
        assert_eq!(window.unleased_seconds, 0);
        assert_window_start_matches_row(&s, &t.id, "clean claim with renewals");
    }

    #[test]
    fn each_lapse_is_derived_with_the_gap_it_left() {
        let s = store();
        let g = goal(&s);
        let t = s.create_task(&g.id, "lapsing", "", None, NOW).unwrap();

        assert!(s.claim_task(&t.id, "alice", 60, NOW).unwrap());
        assert_window_start_matches_row(&s, &t.id, "first claim");

        // Lease ran out at NOW+60; re-claimed at NOW+500.
        assert!(s.claim_task(&t.id, "alice", 60, NOW + 500).unwrap());
        let one = s.claim_window(&t.id).unwrap();
        assert_eq!(one.lapses, 1);
        assert_eq!(one.unleased_seconds, 440);
        assert_window_start_matches_row(&s, &t.id, "after one lapse");

        // Expired again at NOW+560; re-claimed at NOW+1000.
        assert!(s.claim_task(&t.id, "alice", 60, NOW + 1000).unwrap());
        let two = s.claim_window(&t.id).unwrap();
        assert_eq!(two.lapses, 2);
        assert_eq!(two.unleased_seconds, 440 + 440);
        assert!(!two.is_continuous());
        assert_window_start_matches_row(&s, &t.id, "after two lapses");
    }

    #[test]
    fn a_new_owner_opens_a_clean_window_and_does_not_inherit_the_holes() {
        let s = store();
        let g = goal(&s);
        let t = s.create_task(&g.id, "handed over", "", None, NOW).unwrap();

        assert!(s.claim_task(&t.id, "alice", 60, NOW).unwrap());
        assert!(s.claim_task(&t.id, "alice", 60, NOW + 500).unwrap());
        assert_eq!(s.claim_window(&t.id).unwrap().lapses, 1);

        assert!(s.claim_task(&t.id, "bob", 60, NOW + 2000).unwrap());
        let window = s.claim_window(&t.id).unwrap();
        assert!(
            window.is_continuous(),
            "bob did not lapse; alice's holes are not bob's to answer for"
        );
        assert_eq!(window.unleased_seconds, 0);
        assert_window_start_matches_row(&s, &t.id, "after handover");
    }

    /// Parking deliberately clears the lease (ADR-0020), so resuming is not a
    /// lapse — the owner did not lose the task, they set it down. The running
    /// totals never counted this and neither does the derivation.
    #[test]
    fn parking_and_resuming_is_not_a_lapse() {
        let s = store();
        let g = goal(&s);
        let t = s.create_task(&g.id, "parked", "", None, NOW).unwrap();

        assert!(s.claim_task(&t.id, "alice", 600, NOW).unwrap());
        assert!(s
            .pause_task(&t.id, "alice", Some("lunch"), NOW + 10)
            .unwrap());
        assert!(s.resume_task(&t.id, "alice", 600, NOW + 5000).unwrap());

        assert!(s.claim_window(&t.id).unwrap().is_continuous());
        assert_window_start_matches_row(&s, &t.id, "paused then resumed");
    }

    /// ADR-0064 decision 6. A window that opened before the log existed keeps
    /// the lapses it had: deriving from in-log transitions alone would report
    /// zero, and under ADR-0048 zero lapses may certify as aligned — so the
    /// migration would launder a discontinuous window clean.
    #[test]
    fn a_window_imported_from_before_the_log_keeps_the_lapses_it_had() {
        let s = store();
        let g = goal(&s);
        let t = s.create_task(&g.id, "pre-log", "", None, NOW).unwrap();
        assert!(s.claim_task(&t.id, "alice", 600, NOW).unwrap());

        // Reconstruct a genuinely legacy database: the counter columns still
        // present, carrying holes that no recorded transition explains, and no
        // log for this task at all. This is the shape the migration meets in
        // the field, so the seeding path is exercised rather than simulated.
        s.conn
            .execute_batch(
                "ALTER TABLE tasks ADD COLUMN claim_lapses INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE tasks ADD COLUMN unleased_seconds INTEGER NOT NULL DEFAULT 0;",
            )
            .unwrap();
        s.conn
            .execute(
                "UPDATE tasks SET claim_lapses = 3, unleased_seconds = 900 WHERE id = ?1",
                params![t.id],
            )
            .unwrap();
        s.conn
            .execute("DELETE FROM task_events WHERE task_id = ?1", params![t.id])
            .unwrap();

        assert_eq!(import_genesis(&s.conn, NOW + 1).unwrap(), 1);

        let window = s.claim_window(&t.id).unwrap();
        assert_eq!(
            window.lapses, 3,
            "imported holes must survive the migration"
        );
        assert_eq!(window.unleased_seconds, 900);
        assert!(!window.is_continuous());
        assert_window_start_matches_row(&s, &t.id, "imported window");

        // The columns can go now; the holes live in the log.
        s.conn
            .execute_batch(
                "ALTER TABLE tasks DROP COLUMN claim_lapses;
                 ALTER TABLE tasks DROP COLUMN unleased_seconds;",
            )
            .unwrap();
        assert_eq!(s.claim_window(&t.id).unwrap().lapses, 3);

        // And a lapse after the import still accumulates on top of the seed.
        assert!(s.claim_task(&t.id, "alice", 60, NOW + 5000).unwrap());
        assert_eq!(s.claim_window(&t.id).unwrap().lapses, 4);
        assert_window_start_matches_row(&s, &t.id, "imported window, then another lapse");
    }

    /// A database created after ADR-0064 has no counters to carry, and must not
    /// invent any. The genesis then seeds from nothing, which is the honest
    /// answer: there is no pre-log window.
    #[test]
    fn a_genesis_on_a_database_that_never_had_counters_seeds_from_nothing() {
        let s = store();
        let g = goal(&s);
        let t = s.create_task(&g.id, "post-log", "", None, NOW).unwrap();
        assert!(s.claim_task(&t.id, "alice", 600, NOW).unwrap());
        s.conn
            .execute("DELETE FROM task_events WHERE task_id = ?1", params![t.id])
            .unwrap();

        assert_eq!(import_genesis(&s.conn, NOW + 1).unwrap(), 1);

        let genesis = &s.task_events(&t.id).unwrap()[0];
        assert_eq!(genesis.kind, TaskEventKind::Imported);
        assert_eq!(genesis.detail, "", "nothing to carry, so nothing claimed");
        assert!(s.claim_window(&t.id).unwrap().is_continuous());
    }
}

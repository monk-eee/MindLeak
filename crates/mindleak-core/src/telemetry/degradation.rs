//! Which optional work has been failing long enough to be worth interrupting
//! someone about.
//!
//! Separate from the full snapshot because this runs on `open_session`, where a
//! caller is waiting: one narrow query rather than the several a snapshot needs.

use rusqlite::Connection;

use crate::error::Result;

/// How long a tool must go without succeeding before it stops being a blip.
///
/// The autonomous index retries on a bounded exponential backoff, so a single
/// unreachable model produces skips for a while by design. Half an hour is far
/// past that and comfortably past an ordinary restart, which is what keeps this
/// from crying wolf at the start of every session.
const SUSTAINED_AFTER_SECS: i64 = 1_800;

/// A tool whose most recent outcome was `skipped`, and for how long.
pub struct Sustained {
    pub name: String,
    pub last_success_at: Option<i64>,
    pub detail: Option<String>,
}

/// Optional work that has been degraded long enough to have stopped being noise.
///
/// Returned newest-failure-first so the caller can report the worst offender
/// without sorting.
pub fn sustained_degradation(conn: &Connection, now: i64) -> Result<Vec<Sustained>> {
    let mut stmt = conn.prepare(
        "WITH latest AS (
             SELECT name, MAX(id) AS latest_id FROM telemetry_events GROUP BY name
         )
         SELECT event.name,
                MAX(CASE WHEN event.outcome = 'ok' THEN event.ts END) AS last_success_at,
                -- `newest` is already pinned to the one row with this name's
                -- highest id (and that row is confirmed 'skipped' by the join
                -- condition below), so its detail is exactly the most recent
                -- skip's reason. Aggregating over every skip's detail with
                -- MAX(CASE WHEN outcome = 'skipped' THEN detail END) would
                -- instead pick the lexicographically greatest JSON string
                -- across all of this tool's skips, which is not necessarily
                -- the latest one.
                MAX(newest.detail) AS detail
         FROM telemetry_events event
         JOIN latest ON latest.name = event.name
         JOIN telemetry_events newest
           ON newest.id = latest.latest_id AND newest.outcome = 'skipped'
         GROUP BY event.name
         ORDER BY event.name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Sustained {
            name: row.get(0)?,
            last_success_at: row.get(1)?,
            detail: row.get(2)?,
        })
    })?;
    let mut sustained = Vec::new();
    for row in rows {
        let row = row?;
        let long_enough = match row.last_success_at {
            // Never once worked: no amount of waiting makes that transient.
            None => true,
            Some(at) => now.saturating_sub(at) >= SUSTAINED_AFTER_SECS,
        };
        if long_enough {
            sustained.push(row);
        }
    }
    Ok(sustained)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::record;

    fn conn() -> Connection {
        crate::db::open_in_memory().unwrap()
    }

    /// Bug: a degraded tool reported the same after one failed pass and after
    /// four hours, so the autonomous index sat unreachable for 121 consecutive
    /// passes without anyone being told. `open_session` volunteers a stale
    /// binary for exactly this reason; a dead optional feature is no different.
    #[test]
    fn a_long_outage_is_reported_and_a_recent_skip_is_not() {
        let c = conn();
        record(
            &c,
            1_000,
            "maintenance",
            "autonomous_index",
            "ok",
            None,
            None,
        )
        .unwrap();
        record(
            &c,
            1_100,
            "maintenance",
            "autonomous_index",
            "skipped",
            None,
            None,
        )
        .unwrap();

        // Ten minutes after the last success: still inside the backoff's reach.
        assert!(sustained_degradation(&c, 1_600).unwrap().is_empty());

        // An hour after it: long past anything a retry was going to fix.
        let sustained = sustained_degradation(&c, 4_600).unwrap();
        assert_eq!(sustained.len(), 1);
        assert_eq!(sustained[0].name, "autonomous_index");
        assert_eq!(sustained[0].last_success_at, Some(1_000));
    }

    /// A tool that recovered is not degraded at all, however bad it once was.
    #[test]
    fn a_recovered_tool_is_not_reported() {
        let c = conn();
        record(
            &c,
            100,
            "maintenance",
            "autonomous_index",
            "skipped",
            None,
            None,
        )
        .unwrap();
        record(
            &c,
            99_000,
            "maintenance",
            "autonomous_index",
            "ok",
            None,
            None,
        )
        .unwrap();

        assert!(sustained_degradation(&c, 999_999).unwrap().is_empty());
    }

    /// Bug: `detail` was computed as `MAX(CASE WHEN outcome='skipped' THEN
    /// detail END)` across *every* skip this tool ever recorded, and SQLite's
    /// `MAX` over a TEXT column is a lexicographic (byte-wise) comparison, not
    /// a recency one. With two skips recording different reasons, the reason
    /// shown for a sustained outage could be an older skip's message rather
    /// than the tool's actual most recent one -- silently misleading whoever
    /// reads it about why the tool is currently degraded.
    #[test]
    fn detail_reflects_the_most_recent_skip_not_the_lexicographically_largest_one() {
        let c = conn();
        record(
            &c,
            100,
            "maintenance",
            "autonomous_index",
            "skipped",
            None,
            // Lexicographically greatest ("z" > "a"), but the *older* skip.
            Some(&serde_json::json!({ "error": "zzz-earlier-reason" })),
        )
        .unwrap();
        record(
            &c,
            200,
            "maintenance",
            "autonomous_index",
            "skipped",
            None,
            // Lexicographically smallest, but the tool's actual latest reason.
            Some(&serde_json::json!({ "error": "aaa-latest-reason" })),
        )
        .unwrap();

        let sustained = sustained_degradation(&c, 100_000).unwrap();
        assert_eq!(sustained.len(), 1);
        assert!(
            sustained[0]
                .detail
                .as_deref()
                .unwrap()
                .contains("aaa-latest-reason"),
            "expected the most recent skip's detail, got {:?}",
            sustained[0].detail
        );
    }

    /// Never having worked is the strongest case, and subtracting from a
    /// missing success would otherwise measure an age since the epoch.
    #[test]
    fn a_tool_that_never_succeeded_is_reported_immediately() {
        let c = conn();
        record(
            &c,
            100,
            "maintenance",
            "autonomous_index",
            "skipped",
            None,
            None,
        )
        .unwrap();

        let sustained = sustained_degradation(&c, 101).unwrap();
        assert_eq!(sustained.len(), 1);
        assert_eq!(sustained[0].last_success_at, None);
    }
}

//! Deterministic interpretation of usage: high-volume read paths and
//! recommendations, derived from telemetry already loaded for a snapshot.

use super::classify::{is_background_read, is_memory_read};
use super::types::{MemoryHabit, NameMetric, UsageMetric, UsageRetrospective};

const HIGH_VOLUME_READ_THRESHOLD: i64 = 1_000;
const HIGH_VOLUME_READ_LIMIT: usize = 5;

pub(super) fn usage_retrospective(
    by_name: &[NameMetric],
    memory_habits: &[MemoryHabit],
    now: i64,
) -> UsageRetrospective {
    let background_read_calls = by_name
        .iter()
        .filter(|metric| is_background_read(&metric.name))
        .map(|metric| metric.calls)
        .sum();
    let preflight_read_calls = by_name
        .iter()
        .filter(|metric| is_memory_read(&metric.name))
        .map(|metric| metric.calls)
        .sum();
    let architectural_decision_calls = by_name
        .iter()
        .find(|metric| metric.name == "record_architectural_decision")
        .map_or(0, |metric| metric.calls);
    let writing_sessions = memory_habits
        .iter()
        .filter(|habit| habit.attributed_writes > 0)
        .count() as i64;
    let writing_sessions_without_memory_read = memory_habits
        .iter()
        .filter(|habit| habit.read_before_first_write == Some(false))
        .count() as i64;
    let mut high_volume_reads: Vec<UsageMetric> = by_name
        .iter()
        .filter(|metric| {
            is_background_read(&metric.name) && metric.calls >= HIGH_VOLUME_READ_THRESHOLD
        })
        .map(|metric| UsageMetric {
            name: metric.name.clone(),
            calls: metric.calls,
            total_ms: metric.total_ms,
        })
        .collect();
    high_volume_reads.sort_by(|left, right| {
        right
            .calls
            .cmp(&left.calls)
            .then_with(|| left.name.cmp(&right.name))
    });
    high_volume_reads.truncate(HIGH_VOLUME_READ_LIMIT);

    let mut recommendations = Vec::new();
    if writing_sessions_without_memory_read > 0 {
        recommendations.push(format!(
            "{writing_sessions_without_memory_read} of {writing_sessions} writing sessions skipped memory before their first write; run check_overlap on the intended paths before editing."
        ));
    }
    if !high_volume_reads.is_empty() {
        let names = high_volume_reads
            .iter()
            .map(|metric| format!("{} ({})", metric.name, metric.calls))
            .collect::<Vec<_>>()
            .join(", ");
        recommendations.push(format!(
            "High-volume read traffic is dominated by {names}; use visibility-gated, slower refreshes or manual refresh."
        ));
    }
    for metric in by_name.iter().filter(|metric| metric.currently_degraded) {
        let detail = metric
            .last_degraded_detail
            .as_ref()
            .and_then(|value| value.get("error"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("optional work was skipped; inspect telemetry for its configuration");
        recommendations.push(format!(
            "{} {} and is degraded: {detail}",
            metric.name,
            unsuccessful_for(metric, now)
        ));
    }

    UsageRetrospective {
        background_read_calls,
        preflight_read_calls,
        architectural_decision_calls,
        writing_sessions,
        writing_sessions_without_memory_read,
        high_volume_reads,
        recommendations,
    }
}

/// How long a degraded tool has gone without succeeding, in words.
///
/// "is degraded" reads identically after thirty seconds and after four hours,
/// which is how a dead optional feature hides behind a correct signal: the
/// autonomous index skipped 121 consecutive passes across four hours, every
/// report of it indistinguishable from the first, and the only way anyone found
/// out was reading the raw event table. A transient skip and an outage have to
/// say different things or the quiet one is never acted on.
///
/// Derived from the metrics already loaded for the snapshot — no extra query,
/// and no stored "degraded since" that could disagree with the events.
fn unsuccessful_for(metric: &NameMetric, now: i64) -> String {
    match metric.last_success_at {
        // A clock that went backwards is not worth a second sentence about.
        Some(at) if now > at => format!("has not succeeded in {}", approximate_age(now - at)),
        Some(_) => "has not succeeded".to_string(),
        None => "has never succeeded".to_string(),
    }
}

/// Coarse, human units: the reader needs "hours, not seconds", never precision.
fn approximate_age(seconds: i64) -> String {
    match seconds {
        s if s < 120 => format!("{s}s"),
        s if s < 7200 => format!("{}m", s / 60),
        s => format!("{}h{:02}m", s / 3600, (s % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn degraded(name: &str, last_success_at: Option<i64>) -> NameMetric {
        NameMetric {
            name: name.to_string(),
            calls: 1,
            errors: 0,
            total_ms: 1,
            min_ms: 1,
            max_ms: 1,
            avg_ms: 1.0,
            last_success_at,
            last_error_at: None,
            last_error_detail: None,
            last_degraded_at: Some(10),
            last_degraded_detail: Some(json!({ "error": "endpoint unreachable" })),
            currently_failing: false,
            currently_degraded: true,
        }
    }

    fn advice(metrics: &[NameMetric], now: i64) -> Vec<String> {
        usage_retrospective(metrics, &[], now).recommendations
    }

    /// Bug: the recommendation said only "<tool> is degraded", which reads the
    /// same after a blip and after an outage. The autonomous index sat degraded
    /// for four hours over 121 passes and every report of it looked like the
    /// first, so it was found by reading the raw event table rather than by the
    /// signal built for it.
    #[test]
    fn a_long_outage_does_not_read_like_a_passing_blip() {
        let blip = advice(&[degraded("autonomous_index", Some(1_000))], 1_030);
        let outage = advice(&[degraded("autonomous_index", Some(1_000))], 1_000 + 15_120);

        assert_eq!(
            blip,
            vec![
                "autonomous_index has not succeeded in 30s and is degraded: endpoint unreachable"
                    .to_string()
            ]
        );
        assert_eq!(
            outage,
            vec![
                "autonomous_index has not succeeded in 4h12m and is degraded: endpoint unreachable"
                    .to_string()
            ]
        );
    }

    /// A tool that has never once worked is the strongest case there is, and
    /// subtracting from a missing success would have reported an age since the
    /// epoch instead.
    #[test]
    fn a_tool_that_never_worked_says_so_rather_than_inventing_an_age() {
        assert_eq!(
            advice(&[degraded("autonomous_index", None)], 99_999),
            vec![
                "autonomous_index has never succeeded and is degraded: endpoint unreachable"
                    .to_string()
            ]
        );
    }

    /// Reporting only the first degraded tool hides every one after it — the
    /// same shape of defect as the one this function exists to report.
    #[test]
    fn every_degraded_tool_is_reported_not_only_the_first() {
        let reported = advice(
            &[
                degraded("autonomous_index", Some(1_000)),
                degraded("consolidate_session", None),
            ],
            1_060,
        );

        assert_eq!(
            reported,
            vec![
                "autonomous_index has not succeeded in 60s and is degraded: endpoint unreachable"
                    .to_string(),
                "consolidate_session has never succeeded and is degraded: endpoint unreachable"
                    .to_string(),
            ]
        );
    }
}

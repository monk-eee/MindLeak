//! Deterministic interpretation of usage: high-volume read paths and
//! recommendations, derived from telemetry already loaded for a snapshot.

use super::classify::{is_background_read, is_memory_read};
use super::types::{MemoryHabit, NameMetric, UsageMetric, UsageRetrospective};

const HIGH_VOLUME_READ_THRESHOLD: i64 = 1_000;
const HIGH_VOLUME_READ_LIMIT: usize = 5;

pub(super) fn usage_retrospective(
    by_name: &[NameMetric],
    memory_habits: &[MemoryHabit],
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
    if let Some(metric) = by_name.iter().find(|metric| metric.currently_degraded) {
        let detail = metric
            .last_degraded_detail
            .as_ref()
            .and_then(|value| value.get("error"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("optional work was skipped; inspect telemetry for its configuration");
        recommendations.push(format!("{} is degraded: {detail}", metric.name));
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

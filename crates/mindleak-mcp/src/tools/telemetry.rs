use super::{opt_i64, rendered_result};
use mindleak_core::MindLeak;
use serde_json::{json, Value};

pub(super) fn definitions() -> Vec<Value> {
    vec![json!({
        "name": "telemetry_snapshot",
        "description": "Return the observability record (ADR-0010): per-tool call counts, error counts, and latency (min/avg/max ms), plus the most recent tool invocations from this workspace's durable telemetry audit trail. Use this to confirm what actually ran and whether it succeeded.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "limit": { "type": "integer", "default": 20, "minimum": 1, "maximum": 500, "description": "How many recent events to include." }
            }
        }
    })]
}

pub(super) fn dispatch(
    engine: &MindLeak,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    match name {
        "telemetry_snapshot" => Some((|| {
            let limit = opt_i64(args, "limit", 20).clamp(1, 500) as usize;
            let snapshot = engine
                .telemetry_snapshot(limit)
                .map_err(|e| e.to_string())?;
            let mut markdown = format!(
                "**MindLeak telemetry** - {} events, {} errors\n\n| Tool | Calls | Errors | Avg ms | Max ms |\n|---|--:|--:|--:|--:|\n",
                snapshot.total_events, snapshot.total_errors
            );
            for metric in &snapshot.by_name {
                markdown.push_str(&format!(
                    "| {} | {} | {} | {:.1} | {} |\n",
                    metric.name, metric.calls, metric.errors, metric.avg_ms, metric.max_ms
                ));
            }
            Ok(rendered_result(markdown, &json!(snapshot)))
        })()),
        _ => None,
    }
}

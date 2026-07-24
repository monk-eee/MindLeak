use super::{opt_i64, rendered_result};
use mindleak_core::MindLeak;
use serde_json::{json, Value};

pub(super) fn definitions() -> Vec<Value> {
    vec![json!({
        "name": "telemetry_snapshot",
        "description": "Return the observability record (ADR-0010): per-tool lifetime call and error counts, latency (min/avg/max ms), each tool's current health (whether its most recent call failed, with the last error's timestamp and detail), and the most recent tool invocations from this workspace's durable telemetry audit trail. Lifetime error counts are cumulative history and never shrink; use current health to tell whether a tool is failing right now versus has failed at some point in the past.",
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
            let health = if snapshot.currently_failing_tools == 0 {
                "all tools healthy".to_string()
            } else {
                format!(
                    "{} tool(s) currently failing",
                    snapshot.currently_failing_tools
                )
            };
            let mut markdown = format!(
                "**MindLeak telemetry** - {} events, {} lifetime errors, {}\n\nLifetime errors are cumulative history; **Health** reflects each tool's most recent call.\n\n| Tool | Calls | Lifetime errors | Health | Avg ms | Max ms |\n|---|--:|--:|:--|--:|--:|\n",
                snapshot.total_events, snapshot.total_errors, health
            );
            for metric in &snapshot.by_name {
                let tool_health = if metric.currently_failing {
                    "failing"
                } else {
                    "ok"
                };
                markdown.push_str(&format!(
                    "| {} | {} | {} | {} | {:.1} | {} |\n",
                    metric.name,
                    metric.calls,
                    metric.errors,
                    tool_health,
                    metric.avg_ms,
                    metric.max_ms
                ));
            }
            Ok(rendered_result(markdown, &json!(snapshot)))
        })()),
        _ => None,
    }
}

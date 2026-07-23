use super::{opt_f64, opt_i64, req_str, text_result};
use mindleak_core::MindLeak;
use serde_json::{json, Value};

pub(super) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "graph_multi_hop_query",
            "description": "Navigate the context graph up to N directional hops from a seed node id or search phrase, keeping only edges above a minimum time-decayed weight. Returns connected nodes, paths, and recent state.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "seed_entity": { "type": "string", "description": "Node id (e.g. 'artifact:src/auth.ts') or a free-text search phrase." },
                    "max_depth": { "type": "integer", "default": 2, "minimum": 1, "maximum": 6 },
                    "min_weight": { "type": "number", "default": 0.2, "minimum": 0.0, "maximum": 1.0 }
                },
                "required": ["seed_entity"]
            }
        }),
        json!({
            "name": "get_impact_radius",
            "description": "Determine what is structurally connected to a file or symbol you are about to edit: dependent symbols, previously failing executions, and related intents.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target_artifact": { "type": "string", "description": "Node id or path of the file/symbol to assess." }
                },
                "required": ["target_artifact"]
            }
        }),
    ]
}

pub(super) fn dispatch(
    engine: &MindLeak,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    match name {
        "graph_multi_hop_query" => Some((|| {
            let seed = req_str(args, "seed_entity")?;
            let depth = opt_i64(args, "max_depth", 2).clamp(1, 6) as u32;
            let min_weight = opt_f64(args, "min_weight", 0.2);
            let sub = engine
                .multi_hop_query(&seed, depth, min_weight)
                .map_err(|e| e.to_string())?;
            Ok(text_result(&json!(sub)))
        })()),
        "get_impact_radius" => Some((|| {
            let target = req_str(args, "target_artifact")?;
            let sub = engine.impact_radius(&target).map_err(|e| e.to_string())?;
            Ok(text_result(&json!(sub)))
        })()),
        _ => None,
    }
}

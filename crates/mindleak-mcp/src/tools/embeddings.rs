use super::{opt_i64, req_str, text_result};
use mindleak_core::MindLeak;
use serde_json::{json, Value};

pub(super) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "recall",
            "description": "Call for questions about why, prior decisions, regressions, or resumed work. Return meaning-nearest nodes from the optional local embedding index, then seed them into graph_multi_hop_query. If unavailable, use task text with graph_multi_hop_query.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "default": 10, "minimum": 1, "maximum": 100 }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "index",
            "description": "Populate the semantic embedding index for nodes lacking a current vector (off the zero-token hot path). Returns how many nodes were indexed. Optional; requires a local embedding model.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "default": 200, "minimum": 1, "maximum": 5000 }
                }
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
        "recall" => Some((|| {
            let query = req_str(args, "query")?;
            let limit = opt_i64(args, "limit", 10).clamp(1, 100) as usize;
            let results = engine.recall(&query, limit).map_err(|e| e.to_string())?;
            Ok(text_result(&json!({ "results": results })))
        })()),
        "index" => Some((|| {
            let limit = opt_i64(args, "limit", 200).clamp(1, 5000) as usize;
            let indexed = engine.index_nodes(limit).map_err(|e| e.to_string())?;
            Ok(text_result(&json!({ "indexed": indexed })))
        })()),
        _ => None,
    }
}

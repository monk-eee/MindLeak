//! Tool definitions and dispatch for the MindLeak MCP server.

use mindleak_core::ingest::execution::ExecutionRecord;
use mindleak_core::ingest::git::CommitRecord;
use mindleak_core::{now_unix, MindLeak};
use serde_json::{json, Value};

/// The advertised tool list (`tools/list`).
pub fn list() -> Vec<Value> {
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
        json!({
            "name": "record_architectural_decision",
            "description": "Write a high-level architectural decision/tradeoff into the graph as an intent node, linked to the nodes it affects.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "decision_text": { "type": "string" },
                    "related_nodes": { "type": "array", "items": { "type": "string" }, "default": [] }
                },
                "required": ["decision_text"]
            }
        }),
        json!({
            "name": "ingest_execution",
            "description": "Deterministically ingest a terminal command execution: creates an execution node, modified-edges to changed files, and failed-on edges parsed from error output.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "exit_code": { "type": "integer", "default": 0 },
                    "output": { "type": "string", "default": "" },
                    "cwd": { "type": "string" },
                    "changed_files": { "type": "array", "items": { "type": "string" }, "default": [] },
                    "timestamp": { "type": "integer", "description": "Unix seconds; defaults to now." }
                },
                "required": ["command"]
            }
        }),
        json!({
            "name": "ingest_commit",
            "description": "Ingest a git commit as an intent node linked to the artifacts it touched, extracting DECISION/HACK/WHY rationale markers.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message": { "type": "string" },
                    "sha": { "type": "string" },
                    "changed_files": { "type": "array", "items": { "type": "string" }, "default": [] },
                    "timestamp": { "type": "integer" }
                },
                "required": ["message"]
            }
        }),
        json!({
            "name": "ingest_file",
            "description": "Ingest a source file: create an artifact node plus its extracted symbols, linked with contains-edges.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }
        }),
        json!({
            "name": "boost_entity",
            "description": "Record that a node was focused so recency views surface it, without changing incident evidence weights or decay clocks.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }
        }),
        json!({
            "name": "graph_snapshot",
            "description": "Return a visualization subgraph: the neighbourhood of a seed, or the most recently accessed nodes when no seed is given.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "seed": { "type": "string" },
                    "limit": { "type": "integer", "default": 50 }
                }
            }
        }),
        json!({
            "name": "prune_graph",
            "description": "Purge decayed edges and unreferenced execution or symbol nodes.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "graph_stats",
            "description": "Return node count and active-edge count.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "consolidate_session",
            "description": "Optional: compress a batch of raw execution logs into a single intent node using a local, OpenAI-compatible model server (MINDLEAK_LLM_URL / MINDLEAK_MODEL); off the deterministic hot path and errors cleanly if no model is reachable.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "logs": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["logs"]
            }
        }),
        json!({
            "name": "list_agents",
            "description": "List the agent roster: each agent that has ingested or focused nodes, with its active observation count and last-active time. Attribution is recorded only when the server is launched with MINDLEAK_AGENT set.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "recall",
            "description": "Semantic recall: return nodes whose content is closest in meaning to a free-text query, using an optional local embedding index (ADR-0008; MINDLEAK_EMBED_URL / MINDLEAK_EMBED_MODEL). Complements FTS and graph search — seed the results into graph_multi_hop_query. Errors cleanly when no embedding model is reachable.",
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

/// Dispatch a `tools/call` request. Returns a fully-formed MCP result object.
pub fn call(engine: &MindLeak, params: &Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("missing tool name")?;
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);

    match name {
        "graph_multi_hop_query" => {
            let seed = req_str(&args, "seed_entity")?;
            let depth = opt_i64(&args, "max_depth", 2).clamp(1, 6) as u32;
            let min_weight = opt_f64(&args, "min_weight", 0.2);
            let sub = engine
                .multi_hop_query(&seed, depth, min_weight)
                .map_err(|e| e.to_string())?;
            Ok(text_result(&json!(sub)))
        }
        "get_impact_radius" => {
            let target = req_str(&args, "target_artifact")?;
            let sub = engine.impact_radius(&target).map_err(|e| e.to_string())?;
            Ok(text_result(&json!(sub)))
        }
        "record_architectural_decision" => {
            let text = req_str(&args, "decision_text")?;
            let related = str_array(&args, "related_nodes");
            let (id, outcome) = engine
                .record_decision(&text, &related)
                .map_err(|e| e.to_string())?;
            Ok(text_result(&json!({ "intent_id": id, "outcome": outcome })))
        }
        "ingest_execution" => {
            let rec = ExecutionRecord {
                command: req_str(&args, "command")?,
                exit_code: opt_i64(&args, "exit_code", 0) as i32,
                output: opt_str(&args, "output").unwrap_or_default(),
                cwd: opt_str(&args, "cwd"),
                changed_files: str_array(&args, "changed_files"),
                timestamp: opt_i64(&args, "timestamp", now_unix()),
            };
            let outcome = engine.ingest_execution(&rec).map_err(|e| e.to_string())?;
            Ok(text_result(&json!(outcome)))
        }
        "ingest_commit" => {
            let rec = CommitRecord {
                message: req_str(&args, "message")?,
                sha: opt_str(&args, "sha"),
                changed_files: str_array(&args, "changed_files"),
                timestamp: opt_i64(&args, "timestamp", now_unix()),
            };
            let outcome = engine.ingest_commit(&rec).map_err(|e| e.to_string())?;
            Ok(text_result(&json!(outcome)))
        }
        "ingest_file" => {
            let path = req_str(&args, "path")?;
            let content = req_str(&args, "content")?;
            let outcome = engine
                .ingest_file(&path, &content)
                .map_err(|e| e.to_string())?;
            Ok(text_result(&json!(outcome)))
        }
        "boost_entity" => {
            let id = req_str(&args, "id")?;
            let found = engine.boost(&id).map_err(|e| e.to_string())?;
            Ok(text_result(&json!({ "boosted": found, "id": id })))
        }
        "graph_snapshot" => {
            let seed = opt_str(&args, "seed");
            let limit = opt_i64(&args, "limit", 50).clamp(1, 500) as usize;
            let sub = engine
                .snapshot(seed.as_deref(), limit)
                .map_err(|e| e.to_string())?;
            Ok(text_result(&json!(sub)))
        }
        "prune_graph" => {
            let (edges, nodes) = engine.prune().map_err(|e| e.to_string())?;
            Ok(text_result(
                &json!({ "edges_removed": edges, "nodes_removed": nodes }),
            ))
        }
        "graph_stats" => {
            let (nodes, edges) = engine.counts().map_err(|e| e.to_string())?;
            Ok(text_result(
                &json!({ "nodes": nodes, "active_edges": edges }),
            ))
        }
        "consolidate_session" => {
            let logs = str_array(&args, "logs");
            let (id, outcome) = engine
                .consolidate_session(&logs)
                .map_err(|e| e.to_string())?;
            Ok(text_result(&json!({ "intent_id": id, "outcome": outcome })))
        }
        "list_agents" => {
            let agents = engine.list_agents().map_err(|e| e.to_string())?;
            Ok(text_result(&json!({ "agents": agents })))
        }
        "recall" => {
            let query = req_str(&args, "query")?;
            let limit = opt_i64(&args, "limit", 10).clamp(1, 100) as usize;
            let results = engine.recall(&query, limit).map_err(|e| e.to_string())?;
            Ok(text_result(&json!({ "results": results })))
        }
        "index" => {
            let limit = opt_i64(&args, "limit", 200).clamp(1, 5000) as usize;
            let indexed = engine.index_nodes(limit).map_err(|e| e.to_string())?;
            Ok(text_result(&json!({ "indexed": indexed })))
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

fn text_result(value: &Value) -> Value {
    let text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    json!({ "content": [{ "type": "text", "text": text }] })
}

fn req_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("missing required argument: {key}"))
}

fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::to_string)
}

fn opt_i64(args: &Value, key: &str, default: i64) -> i64 {
    args.get(key).and_then(Value::as_i64).unwrap_or(default)
}

fn opt_f64(args: &Value, key: &str, default: f64) -> f64 {
    args.get(key).and_then(Value::as_f64).unwrap_or(default)
}

fn str_array(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    // Generated by AI (UnitTest MCP)
    use super::*;
    use mindleak_core::MindLeak;

    fn call_ok(engine: &MindLeak, name: &str, args: Value) -> Value {
        let params = json!({ "name": name, "arguments": args });
        call(engine, &params).expect("tool call should succeed")
    }

    fn content_text(result: &Value) -> String {
        result["content"][0]["text"].as_str().unwrap().to_string()
    }

    #[test]
    fn list_advertises_all_tools() {
        let names: Vec<String> = list()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert!(names.contains(&"graph_multi_hop_query".to_string()));
        assert!(names.contains(&"get_impact_radius".to_string()));
        assert!(names.contains(&"record_architectural_decision".to_string()));
        assert_eq!(names.len(), 14);
    }

    #[test]
    fn ingest_file_then_query_dispatches() {
        let engine = MindLeak::open_in_memory().unwrap();
        call_ok(
            &engine,
            "ingest_file",
            json!({ "path": "src/auth.ts", "content": "export function validateSession() {}" }),
        );
        let res = call_ok(
            &engine,
            "graph_multi_hop_query",
            json!({ "seed_entity": "src/auth.ts", "max_depth": 2, "min_weight": 0.05 }),
        );
        assert!(content_text(&res).contains("validateSession"));
    }

    #[test]
    fn graph_stats_returns_counts() {
        let engine = MindLeak::open_in_memory().unwrap();
        call_ok(
            &engine,
            "ingest_file",
            json!({ "path": "a.rs", "content": "fn a() {}" }),
        );
        let text = content_text(&call_ok(&engine, "graph_stats", json!({})));
        assert!(text.contains("\"nodes\""));
        assert!(text.contains("\"active_edges\""));
    }

    #[test]
    fn record_decision_returns_intent_id() {
        let engine = MindLeak::open_in_memory().unwrap();
        let res = call_ok(
            &engine,
            "record_architectural_decision",
            json!({ "decision_text": "use sqlite" }),
        );
        assert!(content_text(&res).contains("intent:"));
    }

    #[test]
    fn missing_required_arg_is_error() {
        let engine = MindLeak::open_in_memory().unwrap();
        let params = json!({ "name": "ingest_file", "arguments": { "path": "x.rs" } });
        let err = call(&engine, &params).unwrap_err();
        assert!(err.contains("content"));
    }

    #[test]
    fn unknown_tool_is_error() {
        let engine = MindLeak::open_in_memory().unwrap();
        let params = json!({ "name": "nope", "arguments": {} });
        assert!(call(&engine, &params).unwrap_err().contains("unknown tool"));
    }

    #[test]
    fn consolidate_session_errors_when_model_unreachable() {
        // Point at a dead port so this is deterministic whether or not a real
        // model server happens to be running locally. The tool must error, not panic.
        std::env::set_var("MINDLEAK_LLM_URL", "http://127.0.0.1:1/v1");
        let engine = MindLeak::open_in_memory().unwrap();
        let params = json!({ "name": "consolidate_session", "arguments": { "logs": ["ran test", "failed"] } });
        assert!(call(&engine, &params).is_err());
        std::env::remove_var("MINDLEAK_LLM_URL");
    }
}

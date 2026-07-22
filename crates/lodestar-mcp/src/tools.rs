//! Tool definitions and dispatch for the Lodestar MCP server.

use lodestar_core::{GoalKind, Lodestar};
use serde::Serialize;
use serde_json::{json, Value};

/// The advertised tool list (`tools/list`).
pub fn list() -> Vec<Value> {
    vec![
        // ---- constitution ----
        json!({
            "name": "define_goal",
            "description": "Add a durable constitution entry: an objective, constraint, or invariant that governs the work. Read the constitution before acting.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["objective", "constraint", "invariant"] },
                    "title": { "type": "string" },
                    "statement": { "type": "string", "description": "The normative text: what must hold or be achieved." },
                    "parent_id": { "type": "string", "description": "Optional parent goal id for hierarchy." }
                },
                "required": ["kind", "title", "statement"]
            }
        }),
        json!({
            "name": "supersede_goal",
            "description": "Replace a goal with a new active version (the old one is retired, not deleted). The only way intent changes — explicit and attributed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "goal_id": { "type": "string" },
                    "new_statement": { "type": "string" },
                    "reason": { "type": "string" }
                },
                "required": ["goal_id", "new_statement", "reason"]
            }
        }),
        json!({
            "name": "get_constitution",
            "description": "Return the active goals, constraints, and invariants — the authoritative intent every agent should read before acting.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "link_goal_to_code",
            "description": "Link a goal to the MindLeak code nodes (artifact:/symbol: ids) that realise it, so conformance can tell which intent governs a file.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "goal_id": { "type": "string" },
                    "node_ids": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["goal_id", "node_ids"]
            }
        }),
        json!({
            "name": "export_constitution",
            "description": "Render the active constitution as committed-friendly markdown; optionally write it to a path for review in a PR.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string", "description": "Optional file path to write." } }
            }
        }),
        // ---- executive ----
        json!({
            "name": "create_task",
            "description": "Create a claimable task serving a goal.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "goal_id": { "type": "string" },
                    "title": { "type": "string" },
                    "acceptance": { "type": "string", "description": "What 'done' means." }
                },
                "required": ["goal_id", "title"]
            }
        }),
        json!({
            "name": "decompose_goal",
            "description": "Break a goal into claimable tasks (uses a local model when reachable, else a single-task fallback).",
            "inputSchema": {
                "type": "object",
                "properties": { "goal_id": { "type": "string" } },
                "required": ["goal_id"]
            }
        }),
        json!({
            "name": "next_task",
            "description": "Suggest the next unblocked, claimable task (open or lease-expired), oldest first.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "claim_task",
            "description": "Atomically claim a task with a lease (TTL seconds). Returns won=true only if this agent won the race — the coordination primitive that stops parallel agents colliding.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "agent": { "type": "string" },
                    "lease_secs": { "type": "integer", "default": 300 }
                },
                "required": ["task_id", "agent"]
            }
        }),
        json!({
            "name": "renew_lease",
            "description": "Heartbeat: extend the lease on a task this agent still owns.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "agent": { "type": "string" },
                    "lease_secs": { "type": "integer", "default": 300 }
                },
                "required": ["task_id", "agent"]
            }
        }),
        json!({
            "name": "complete_task",
            "description": "Complete a task (owner-guarded), then run conformance on the code its goal governs. A violation blocks the task instead of completing it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "agent": { "type": "string" }
                },
                "required": ["task_id", "agent"]
            }
        }),
        json!({
            "name": "release_task",
            "description": "Release a claim back to open (owner-guarded).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "agent": { "type": "string" }
                },
                "required": ["task_id", "agent"]
            }
        }),
        json!({
            "name": "block_task",
            "description": "Mark a task blocked, optionally on another task.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "blocked_by": { "type": "string" }
                },
                "required": ["task_id"]
            }
        }),
        json!({
            "name": "board",
            "description": "The live coordination snapshot: every task with owner, status, and lease.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        // ---- conformance ----
        json!({
            "name": "check_conformance",
            "description": "Check whether a set of changed code nodes conforms to governing intent: aligned, drift (governed code changed with no covering task), or violation.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "change_node_ids": { "type": "array", "items": { "type": "string" } },
                    "task_id": { "type": "string", "description": "Optional covering task." }
                },
                "required": ["change_node_ids"]
            }
        }),
        // ---- knowledge / consolidation ----
        json!({
            "name": "record_knowledge",
            "description": "Record a consolidated learned regularity (durable but revalidated). Prefer 'consolidate' for gated promotion.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "statement": { "type": "string" },
                    "evidence": { "type": "string", "description": "JSON provenance." },
                    "half_life_hours": { "type": "number" }
                },
                "required": ["statement"]
            }
        }),
        json!({
            "name": "consolidate",
            "description": "Gated promotion of a discovered regularity into durable knowledge. Stores nothing unless the evidence clears count + span thresholds (signal, not coincidence).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "statement": { "type": "string" },
                    "evidence_node_ids": { "type": "array", "items": { "type": "string" } },
                    "first_seen": { "type": "integer" },
                    "last_seen": { "type": "integer" }
                },
                "required": ["statement", "evidence_node_ids", "first_seen", "last_seen"]
            }
        }),
        json!({
            "name": "active_knowledge",
            "description": "Return learned knowledge still above the active threshold, strongest first.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "reconfirm_knowledge",
            "description": "Re-confirm a knowledge node with fresh evidence (resets its revalidation clock).",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }
        }),
        json!({
            "name": "prune_knowledge",
            "description": "Purge knowledge that decayed below the threshold without reconfirmation.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "lodestar_stats",
            "description": "Counts: active goals, open/claimed/done tasks, active knowledge.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
    ]
}

/// Dispatch a `tools/call`. Returns the MCP `content` object or an error string.
pub fn call(engine: &Lodestar, params: &Value) -> Result<Value, String> {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);

    match name {
        "define_goal" => {
            let kind = parse_kind(req_str(&args, "kind")?)?;
            let goal = engine
                .define_goal(
                    kind,
                    req_str(&args, "title")?,
                    req_str(&args, "statement")?,
                    opt_str(&args, "parent_id"),
                )
                .map_err(|e| e.to_string())?;
            ok(&goal)
        }
        "supersede_goal" => {
            let goal = engine
                .supersede_goal(
                    req_str(&args, "goal_id")?,
                    req_str(&args, "new_statement")?,
                    req_str(&args, "reason")?,
                )
                .map_err(|e| e.to_string())?;
            ok(&goal)
        }
        "get_constitution" => ok(&engine.get_constitution().map_err(|e| e.to_string())?),
        "link_goal_to_code" => {
            let linked = engine
                .link_goal_to_code(req_str(&args, "goal_id")?, &str_array(&args, "node_ids"))
                .map_err(|e| e.to_string())?;
            ok(&json!({ "linked": linked }))
        }
        "export_constitution" => {
            let md = engine
                .export_constitution(opt_str(&args, "path").as_deref())
                .map_err(|e| e.to_string())?;
            text(md)
        }
        "create_task" => {
            let task = engine
                .create_task(
                    req_str(&args, "goal_id")?,
                    req_str(&args, "title")?,
                    opt_str(&args, "acceptance").unwrap_or_default().as_str(),
                )
                .map_err(|e| e.to_string())?;
            ok(&task)
        }
        "decompose_goal" => ok(&engine
            .decompose_goal(req_str(&args, "goal_id")?)
            .map_err(|e| e.to_string())?),
        "next_task" => match engine.next_task().map_err(|e| e.to_string())? {
            Some(t) => ok(&t),
            None => text("no claimable task".to_string()),
        },
        "claim_task" => {
            let won = engine
                .claim_task(
                    req_str(&args, "task_id")?,
                    req_str(&args, "agent")?,
                    i64_arg(&args, "lease_secs", 300),
                )
                .map_err(|e| e.to_string())?;
            ok(&json!({ "won": won }))
        }
        "renew_lease" => {
            let renewed = engine
                .renew_lease(
                    req_str(&args, "task_id")?,
                    req_str(&args, "agent")?,
                    i64_arg(&args, "lease_secs", 300),
                )
                .map_err(|e| e.to_string())?;
            ok(&json!({ "renewed": renewed }))
        }
        "complete_task" => {
            let (completed, conformance) = engine
                .complete_task(req_str(&args, "task_id")?, req_str(&args, "agent")?)
                .map_err(|e| e.to_string())?;
            ok(&json!({ "completed": completed, "conformance": conformance }))
        }
        "release_task" => {
            let released = engine
                .release_task(req_str(&args, "task_id")?, req_str(&args, "agent")?)
                .map_err(|e| e.to_string())?;
            ok(&json!({ "released": released }))
        }
        "block_task" => {
            let blocked = engine
                .block_task(req_str(&args, "task_id")?, opt_str(&args, "blocked_by"))
                .map_err(|e| e.to_string())?;
            ok(&json!({ "blocked": blocked }))
        }
        "board" => ok(&engine.board().map_err(|e| e.to_string())?),
        "check_conformance" => {
            let result = engine
                .check_conformance(
                    &str_array(&args, "change_node_ids"),
                    opt_str(&args, "task_id").as_deref(),
                )
                .map_err(|e| e.to_string())?;
            ok(&result)
        }
        "record_knowledge" => {
            let k = engine
                .record_knowledge(
                    req_str(&args, "statement")?,
                    opt_str(&args, "evidence")
                        .unwrap_or_else(|| "{}".to_string())
                        .as_str(),
                    args.get("half_life_hours").and_then(Value::as_f64),
                )
                .map_err(|e| e.to_string())?;
            ok(&k)
        }
        "consolidate" => {
            let promoted = engine
                .consolidate(
                    req_str(&args, "statement")?,
                    &str_array(&args, "evidence_node_ids"),
                    i64_arg(&args, "first_seen", 0),
                    i64_arg(&args, "last_seen", 0),
                )
                .map_err(|e| e.to_string())?;
            match promoted {
                Some(k) => ok(&k),
                None => text("not promoted: evidence below count/span threshold".to_string()),
            }
        }
        "active_knowledge" => ok(&engine.active_knowledge().map_err(|e| e.to_string())?),
        "reconfirm_knowledge" => {
            let reconfirmed = engine
                .reconfirm_knowledge(req_str(&args, "id")?)
                .map_err(|e| e.to_string())?;
            ok(&json!({ "reconfirmed": reconfirmed }))
        }
        "prune_knowledge" => {
            let pruned = engine.prune_knowledge().map_err(|e| e.to_string())?;
            ok(&json!({ "pruned": pruned }))
        }
        "lodestar_stats" => ok(&engine.stats().map_err(|e| e.to_string())?),
        other => Err(format!("unknown tool: {other}")),
    }
}

// ---- helpers ---------------------------------------------------------------

fn parse_kind(s: &str) -> Result<GoalKind, String> {
    GoalKind::from_tag(s).ok_or_else(|| format!("invalid kind: {s}"))
}

fn req_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing required string arg: {key}"))
}

fn opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn i64_arg(args: &Value, key: &str, default: i64) -> i64 {
    args.get(key).and_then(Value::as_i64).unwrap_or(default)
}

fn str_array(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn ok<T: Serialize>(value: &T) -> Result<Value, String> {
    let body = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    text(body)
}

fn text(body: String) -> Result<Value, String> {
    Ok(json!({ "content": [{ "type": "text", "text": body }] }))
}

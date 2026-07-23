//! Constitution tool definitions and dispatch.

use super::{ok, opt_str, req_str, str_array, text};
use lodestar_core::{CodeBindingMode, GoalKind, Lodestar};
use serde_json::{json, Value};

pub(super) fn definitions() -> Vec<Value> {
    vec![
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
                    "node_ids": { "type": "array", "items": { "type": "string" } },
                    "mode": { "type": "string", "enum": ["governed", "forbid_change"], "default": "governed" }
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
    ]
}

pub(super) fn dispatch(
    engine: &Lodestar,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    match name {
        "define_goal" => Some((|| {
            let kind = parse_kind(req_str(args, "kind")?)?;
            let goal = engine
                .define_goal(
                    kind,
                    req_str(args, "title")?,
                    req_str(args, "statement")?,
                    opt_str(args, "parent_id"),
                )
                .map_err(|e| e.to_string())?;
            ok(&goal)
        })()),
        "supersede_goal" => Some((|| {
            let goal = engine
                .supersede_goal(
                    req_str(args, "goal_id")?,
                    req_str(args, "new_statement")?,
                    req_str(args, "reason")?,
                )
                .map_err(|e| e.to_string())?;
            ok(&goal)
        })()),
        "get_constitution" => Some((|| {
            ok(&engine.get_constitution().map_err(|e| e.to_string())?)
        })()),
        "link_goal_to_code" => Some((|| {
            let mode = parse_binding_mode(
                opt_str(args, "mode")
                    .unwrap_or_else(|| "governed".to_string())
                    .as_str(),
            )?;
            let linked = engine
                .link_goal_to_code(
                    req_str(args, "goal_id")?,
                    &str_array(args, "node_ids"),
                    mode,
                )
                .map_err(|e| e.to_string())?;
            ok(&json!({ "linked": linked }))
        })()),
        "export_constitution" => Some((|| {
            let md = engine
                .export_constitution(opt_str(args, "path").as_deref())
                .map_err(|e| e.to_string())?;
            text(md)
        })()),
        _ => None,
    }
}

fn parse_kind(s: &str) -> Result<GoalKind, String> {
    GoalKind::from_tag(s).ok_or_else(|| format!("invalid kind: {s}"))
}

fn parse_binding_mode(value: &str) -> Result<CodeBindingMode, String> {
    CodeBindingMode::from_tag(value).ok_or_else(|| format!("invalid code binding mode: {value}"))
}

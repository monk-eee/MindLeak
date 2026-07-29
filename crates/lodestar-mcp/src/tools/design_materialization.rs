//! The reviewed materialization plan: its schema, and how it is parsed.
//!
//! The tools that use it are `design_promote` and `design_query` in
//! [`super::design`]; this module owns the payload shape they share, which is
//! large enough that inlining it would bury the four verbs it belongs to.

use lodestar_core::DesignMaterializationPlan;
use serde_json::{json, Value};

pub(super) fn parse_materialization_plan(
    args: &Value,
) -> Result<DesignMaterializationPlan, String> {
    let plan = args
        .get("plan")
        .cloned()
        .ok_or_else(|| "missing required object arg: plan".to_string())?;
    serde_json::from_value(plan).map_err(|error| format!("invalid materialization plan: {error}"))
}

pub(super) fn materialization_plan_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "mode": { "type": "string", "enum": ["create", "link", "no_work"] },
            "tasks": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "goal_id": { "type": "string" },
                        "title": { "type": "string" },
                        "acceptance": { "type": "string" }
                    },
                    "required": ["goal_id", "title", "acceptance"]
                }
            },
            "task_ids": { "type": "array", "items": { "type": "string" }, "uniqueItems": true },
            "constraints": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "kind": { "type": "string", "enum": ["constraint", "invariant"] },
                        "title": { "type": "string" },
                        "statement": { "type": "string" }
                    },
                    "required": ["kind", "title", "statement"]
                }
            },
            "rationale": { "type": "string" }
        },
        "required": ["mode"]
    })
}

//! MCP tools for reviewed design materialization and repair.

use lodestar_core::{DesignMaterializationPlan, Lodestar};
use serde_json::{json, Value};

use super::{ok, req_str};

pub(super) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "design_promotion",
            "description": "Read the current objectives, tasks, and constraints materialized for a design. Returns null while proposed or pending; never invokes planning.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }
        }),
        json!({
            "name": "design_materialization_history",
            "description": "Read every immutable reviewed materialization and repair revision for a design, oldest first.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }
        }),
        json!({
            "name": "plan_design_promotion",
            "description": "Produce a read-only suggested create plan for an accepted design under one objective. The caller must show and review this plan before materializing it; this tool never creates tasks.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "objective_goal_id": { "type": "string" }
                },
                "required": ["id", "objective_goal_id"]
            }
        }),
        json!({
            "name": "promote_design",
            "description": "Materialize exactly one explicit human-reviewed create/link/no-work plan for an accepted design. Idempotent retries return the same revision and never duplicate work.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Accepted design item id." },
                    "plan": materialization_plan_schema()
                },
                "required": ["id", "plan"]
            }
        }),
        json!({
            "name": "revise_design_promotion",
            "description": "Append an attributed repair revision for a materialized design and replace its current provenance projection. Prior plans and tasks remain durable; a non-empty rationale is required.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "human": { "type": "string" },
                    "plan": materialization_plan_schema()
                },
                "required": ["id", "human", "plan"]
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
        "design_promotion" => Some((|| {
            ok(&engine
                .design_promotion(req_str(args, "id")?)
                .map_err(|error| error.to_string())?)
        })()),
        "design_materialization_history" => Some((|| {
            ok(&engine
                .design_materialization_history(req_str(args, "id")?)
                .map_err(|error| error.to_string())?)
        })()),
        "plan_design_promotion" => Some((|| {
            ok(&engine
                .plan_design_promotion(req_str(args, "id")?, req_str(args, "objective_goal_id")?)
                .map_err(|error| error.to_string())?)
        })()),
        "promote_design" => Some((|| {
            let plan = parse_materialization_plan(args)?;
            ok(&engine
                .promote_design(req_str(args, "id")?, &plan)
                .map_err(|error| error.to_string())?)
        })()),
        "revise_design_promotion" => Some((|| {
            let plan = parse_materialization_plan(args)?;
            ok(&engine
                .revise_design_promotion(req_str(args, "id")?, req_str(args, "human")?, &plan)
                .map_err(|error| error.to_string())?)
        })()),
        _ => None,
    }
}

fn parse_materialization_plan(args: &Value) -> Result<DesignMaterializationPlan, String> {
    let plan = args
        .get("plan")
        .cloned()
        .ok_or_else(|| "missing required object arg: plan".to_string())?;
    serde_json::from_value(plan).map_err(|error| format!("invalid materialization plan: {error}"))
}

fn materialization_plan_schema() -> Value {
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

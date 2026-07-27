//! Waiver tool definitions and dispatch (SPEC-CONSTITUTION §9).

use super::{i64_arg, ok, opt_str, req_str};
use lodestar_core::waiver::WaiverRequest;
use lodestar_core::Lodestar;
use serde_json::{json, Value};

pub(super) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "grant_waiver",
            "description": "Grant a scoped, expiring, attributed exception to one clause — the reviewable form of the thing an agent would otherwise do with --no-verify. Approved by the calling session, so an agent cannot approve an exception to a clause that names a human authority. Refuses an unwaivable clause, a wrong approver, and any expiry that is not in the future: a permanent exception is an amendment, not a waiver.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clause_id": { "type": "string" },
                    "scope": { "type": "string", "description": "What the exception covers: an artifact:/symbol:/workflow: token, or a prefix ending ** for everything beneath it." },
                    "reason": { "type": "string", "description": "Why the exception is justified — read at renewal time, so write it for someone else." },
                    "expires_at": { "type": "integer", "description": "Unix seconds. Required and must be in the future." },
                    "remediation_task_id": { "type": "string", "description": "The work that makes the exception unnecessary." }
                },
                "required": ["clause_id", "scope", "reason", "expires_at"]
            }
        }),
        json!({
            "name": "revoke_waiver",
            "description": "Withdraw a waiver. Immediate for future checks and never retroactive: prior conformance records keep the verdict they were given under the policy in force at the time. The row is kept, not deleted — the exception happened.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "waiver_id": { "type": "string" },
                    "reason": { "type": "string" }
                },
                "required": ["waiver_id", "reason"]
            }
        }),
        json!({
            "name": "clause_waivers",
            "description": "Every waiver ever granted against one clause, including lapsed and revoked ones. How often a rule has been excepted is usually the more useful question than what is excepted right now — a clause waived repeatedly is a clause that wants amending.",
            "inputSchema": {
                "type": "object",
                "properties": { "clause_id": { "type": "string" } },
                "required": ["clause_id"]
            }
        }),
        json!({
            "name": "active_waivers",
            "description": "Every exception currently in force, soonest to expire first — what is not being enforced right now, and until when.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
    ]
}

pub(super) fn dispatch(
    engine: &Lodestar,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    match name {
        "grant_waiver" => Some(grant_waiver(engine, args)),
        "revoke_waiver" => Some(revoke_waiver(engine, args)),
        "clause_waivers" => Some(clause_waivers(engine, args)),
        "active_waivers" => Some(active_waivers(engine)),
        _ => None,
    }
}

fn grant_waiver(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let granted = engine
        .grant_waiver(&WaiverRequest {
            clause_id: req_str(args, "clause_id")?.to_string(),
            scope: req_str(args, "scope")?.to_string(),
            reason: req_str(args, "reason")?.to_string(),
            approved_by: req_str(args, "agent")?.to_string(),
            expires_at: i64_arg(args, "expires_at", 0),
            remediation_task_id: opt_str(args, "remediation_task_id"),
        })
        .map_err(|e| e.to_string())?;
    ok(&json!({
        "waiver_id": granted.id,
        "clause_id": granted.clause_id,
        "scope": granted.scope,
        "approved_by": granted.approved_by,
        "expires_at": granted.expires_at,
        "remediation_task_id": granted.remediation_task_id,
        "constitution_version": granted.constitution_version,
    }))
}

fn revoke_waiver(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let revoked = engine
        .revoke_waiver(
            req_str(args, "waiver_id")?,
            req_str(args, "agent")?,
            req_str(args, "reason")?,
        )
        .map_err(|e| e.to_string())?;
    ok(&json!({
        "waiver_id": revoked.id,
        "status": revoked.status.as_str(),
        "revoked_by": revoked.revoked_by,
        "revoked_at": revoked.revoked_at,
    }))
}

fn clause_waivers(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let waivers = engine
        .clause_waivers(req_str(args, "clause_id")?)
        .map_err(|e| e.to_string())?;
    ok(&json!({ "waivers": waivers }))
}

fn active_waivers(engine: &Lodestar) -> Result<Value, String> {
    let waivers = engine.live_waivers().map_err(|e| e.to_string())?;
    ok(&json!({ "waivers": waivers }))
}

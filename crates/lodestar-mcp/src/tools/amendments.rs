//! Amendment and pack-upgrade tool definitions and dispatch
//! (SPEC-CONSTITUTION §9).

use super::{ok, opt_str, req_str};
use lodestar_core::{GoalKind, Lodestar};
use serde_json::{json, Value};

pub(super) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "propose_amendment",
            "description": "Begin changing adopted policy: draft the next constitutional version, carrying every active clause forward so the draft starts as the current policy. Edit the draft, then amend_constitution promotes it. Starting from a copy is what keeps the eventual diff readable — only what you actually change appears in it. Refuses when no constitution is active (that is an activation) or a draft is already open.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "draft_clause",
            "description": "Author a NEW rule into an open amendment draft, then give it a contract with complete_clause_contract before amend_constitution promotes it. This is how policy grows: define_goal states a rule that is live the moment it is written, and complete_clause_contract refuses to harden a live rule (that is what an amendment is for), so without this the clause most needing an enforcement contract was the one clause that could never be given one. The clause takes effect only if the draft is promoted, and appears in constitution_diff as 'added' for the reviewer. Use this rather than minting a policy pack for a rule this project wrote itself — a pack records immutable upstream provenance, which would be a fabricated source.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "draft_id": { "type": "string", "description": "The draft opened by propose_amendment." },
                    "kind": { "type": "string", "enum": ["objective", "constraint", "invariant"] },
                    "title": { "type": "string" },
                    "statement": { "type": "string", "description": "The normative text: what must hold." }
                },
                "required": ["draft_id", "kind", "title", "statement"]
            }
        }),
        json!({
            "name": "amend_constitution",
            "description": "Promote a reviewed amendment draft, retiring the version it replaces and recording an attributed rationale plus an explicit clause diff. The old version and its clauses are superseded, never deleted, so prior conformance records keep naming the policy they were judged under. `approved_by` names who accepted the change and must differ from the calling agent: an agent may approve an amendment, just not its own, which is what lets the audit history tell a reviewed adoption from an agent changing policy alone. Attributed, not authenticated (ADR-0071). Refuses an amendment that changes nothing, one that leaves no clauses at all, and one carrying an undecided clause proposal.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "draft_id": { "type": "string" },
                    "approved_by": { "type": "string", "description": "Who accepted this change. Must differ from the calling agent; another agent is allowed, approving your own is not." },
                    "rationale": { "type": "string", "description": "Why the rule is changing — the thing a reader will want most in a year." }
                },
                "required": ["draft_id", "approved_by", "rationale"]
            }
        }),
        json!({
            "name": "constitution_diff",
            "description": "The clause-level difference between two constitutional versions, changing nothing — what an amendment would do. Clauses match on slug, so a restated rule reads as changed rather than as a removal plus an addition, and a clause that only hardens its scope or consequence still shows up.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from_version": { "type": "string" },
                    "to_version": { "type": "string" }
                },
                "required": ["from_version", "to_version"]
            }
        }),
        json!({
            "name": "amendments",
            "description": "The amendment history, newest first — how policy got to where it is, with each rationale and stored diff.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "plan_pack_upgrade",
            "description": "Compare a newer pack version against what this project actually adopted from it. A proposal, never an upgrade: an upstream version change can never alter active local policy, so this is a pure read that produces the argument for amending. Clauses you tailored locally are flagged, because accepting an upstream change to one would silently discard a deliberate local decision.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pack_id": { "type": "string" },
                    "to_version": { "type": "string" }
                },
                "required": ["pack_id", "to_version"]
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
        "propose_amendment" => Some(propose_amendment(engine, args)),
        "draft_clause" => Some(draft_clause(engine, args)),
        "amend_constitution" => Some(amend_constitution(engine, args)),
        "constitution_diff" => Some(constitution_diff(engine, args)),
        "amendments" => Some(amendments(engine)),
        "plan_pack_upgrade" => Some(plan_pack_upgrade(engine, args)),
        _ => None,
    }
}

fn propose_amendment(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let version = engine
        .propose_amendment(opt_str(args, "agent").as_deref())
        .map_err(|e| e.to_string())?;
    ok(&json!({
        "draft_id": version.id,
        "version": version.version,
        "status": version.status.as_str(),
        "note": "carried the active clauses forward; edit the draft, then amend_constitution",
    }))
}

fn draft_clause(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let kind = GoalKind::from_tag(req_str(args, "kind")?).ok_or_else(|| {
        format!(
            "invalid kind: {}",
            req_str(args, "kind").unwrap_or_default()
        )
    })?;
    let clause = engine
        .draft_clause(
            req_str(args, "draft_id")?,
            kind,
            req_str(args, "title")?,
            req_str(args, "statement")?,
        )
        .map_err(|e| e.to_string())?;
    ok(&json!({
        "clause_id": clause.id,
        "slug": clause.slug,
        "status": clause.status.as_str(),
        "constitution_version": clause.constitution_version,
        "note": "authored into the draft; give it a contract with complete_clause_contract, \
                 then amend_constitution promotes it",
    }))
}

fn amend_constitution(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let amendment = engine
        .amend_constitution(
            req_str(args, "draft_id")?,
            req_str(args, "agent")?,
            req_str(args, "approved_by")?,
            req_str(args, "rationale")?,
        )
        .map_err(|e| e.to_string())?;
    ok(&json!({
        "amendment_id": amendment.id,
        "from_version": amendment.from_version,
        "to_version": amendment.to_version,
        "amended_by": amendment.amended_by,
        "approved_by": amendment.approved_by,
        "diff": amendment.diff,
    }))
}

fn constitution_diff(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let diff = engine
        .constitution_diff(req_str(args, "from_version")?, req_str(args, "to_version")?)
        .map_err(|e| e.to_string())?;
    ok(&json!({ "diff": diff }))
}

fn amendments(engine: &Lodestar) -> Result<Value, String> {
    let history = engine.amendments().map_err(|e| e.to_string())?;
    ok(&json!({ "amendments": history }))
}

fn plan_pack_upgrade(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let plan = engine
        .plan_pack_upgrade(req_str(args, "pack_id")?, req_str(args, "to_version")?)
        .map_err(|e| e.to_string())?;
    ok(&json!(plan))
}

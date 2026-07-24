//! Constitution tool definitions and dispatch.

use super::{bool_arg, ok, opt_str, req_str, str_array, text};
use lodestar_core::{
    CodeBindingMode, ConstitutionPack, GoalKind, Lodestar, PackClause, PackClauseDisposition,
};
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
            "name": "unlink_goal_from_code",
            "description": "Remove goal↔code bindings — the inverse of link_goal_to_code. Prune a stale or mistaken binding (e.g. a shared doc, or a source file a goal no longer realises) so conformance stops flagging honest changes to it as drift against that goal. A node not bound to the goal is a no-op. Returns how many bindings were removed.",
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
            "name": "governing_goals",
            "description": "Audit which active goals govern a code node, and how (governed / forbid_change) — inspect binding hygiene before pruning with unlink_goal_from_code.",
            "inputSchema": {
                "type": "object",
                "properties": { "node_id": { "type": "string" } },
                "required": ["node_id"]
            }
        }),
        json!({
            "name": "governing_for_task",
            "description": "Return the active clauses governing a task's linked scope (the code its goal is bound to), each with its goal and binding mode — so an agent or the Intent Board sees what governs the work an agent picked up, without a separate advise call (ADR-0029). Bounded and deduped by clause.",
            "inputSchema": {
                "type": "object",
                "properties": { "task_id": { "type": "string" } },
                "required": ["task_id"]
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
        json!({
            "name": "register_policy_pack",
            "description": "Validate and register one immutable policy-pack version. Same id/version/digest is idempotent; different content under an existing version is refused.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pack": { "type": "object", "description": "ConstitutionPack including its canonical SHA-256 digest." }
                },
                "required": ["pack"]
            }
        }),
        json!({
            "name": "propose_policy_pack",
            "description": "Create durable review proposals for every undecided clause in an immutable policy pack. Declared conflicts return needs_human; rejected clauses are not re-proposed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pack_id": { "type": "string" },
                    "version": { "type": "string" },
                    "constitution_version": { "type": "string", "description": "Optional explicit draft/active constitution id; defaults to the active version." }
                },
                "required": ["pack_id", "version"]
            }
        }),
        json!({
            "name": "propose_common_core",
            "description": "Register and propose the five review-first Common Core principles (evidence, intent, safety, proportionality, evolution). They are proposals, never implicit law.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "list_pack_proposals",
            "description": "List policy-pack clause proposals for one pack/version and constitution context.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "pack_id": { "type": "string" },
                    "version": { "type": "string" },
                    "constitution_version": { "type": "string" },
                    "include_decided": { "type": "boolean", "default": false }
                },
                "required": ["pack_id", "version"]
            }
        }),
        json!({
            "name": "review_pack_clause",
            "description": "Attribute one human review disposition (adopted, tailored, or rejected) to a pack-clause proposal. Adoption copies a self-contained local clause plus immutable source provenance; conflicts and pack upgrades require explicit later resolution.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "proposal_id": { "type": "string" },
                    "disposition": { "type": "string", "enum": ["adopted", "tailored", "rejected"] },
                    "tailored_clause": { "type": "object", "description": "Required only for tailored; must preserve the source clause key." },
                    "reason": { "type": "string", "description": "Required for rejection; recommended for tailoring." },
                    "agent": { "type": "string", "description": "Injected from the registered session." }
                },
                "required": ["proposal_id", "disposition", "agent"]
            }
        }),
        json!({
            "name": "pack_clause_provenance",
            "description": "Resolve an adopted local goal back to its immutable source pack id, version, digest, key, and original clause.",
            "inputSchema": {
                "type": "object",
                "properties": { "goal_id": { "type": "string" } },
                "required": ["goal_id"]
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
        "unlink_goal_from_code" => Some((|| {
            let removed = engine
                .unlink_goal_from_code(req_str(args, "goal_id")?, &str_array(args, "node_ids"))
                .map_err(|e| e.to_string())?;
            ok(&json!({ "removed": removed }))
        })()),
        "governing_goals" => Some((|| {
            ok(&engine
                .governing_goals(req_str(args, "node_id")?)
                .map_err(|e| e.to_string())?)
        })()),
        "governing_for_task" => Some((|| {
            ok(&engine
                .governing_clauses_for_task(req_str(args, "task_id")?)
                .map_err(|e| e.to_string())?)
        })()),
        "export_constitution" => Some((|| {
            let md = engine
                .export_constitution(opt_str(args, "path").as_deref())
                .map_err(|e| e.to_string())?;
            text(md)
        })()),
        "register_policy_pack" => Some((|| {
            let pack: ConstitutionPack = serde_json::from_value(
                args.get("pack")
                    .cloned()
                    .ok_or_else(|| "missing required object arg: pack".to_string())?,
            )
            .map_err(|error| format!("invalid policy pack: {error}"))?;
            ok(&engine
                .register_policy_pack(&pack)
                .map_err(|error| error.to_string())?)
        })()),
        "propose_policy_pack" => Some((|| {
            ok(&engine
                .propose_policy_pack(
                    req_str(args, "pack_id")?,
                    req_str(args, "version")?,
                    opt_str(args, "constitution_version").as_deref(),
                )
                .map_err(|error| error.to_string())?)
        })()),
        "propose_common_core" => Some((|| {
            ok(&engine
                .propose_common_core()
                .map_err(|error| error.to_string())?)
        })()),
        "list_pack_proposals" => Some((|| {
            ok(&engine
                .policy_pack_proposals(
                    req_str(args, "pack_id")?,
                    req_str(args, "version")?,
                    opt_str(args, "constitution_version").as_deref(),
                    bool_arg(args, "include_decided", false),
                )
                .map_err(|error| error.to_string())?)
        })()),
        "review_pack_clause" => Some((|| {
            let disposition = PackClauseDisposition::from_tag(req_str(args, "disposition")?)
                .ok_or_else(|| "disposition must be adopted, tailored, or rejected".to_string())?;
            let tailored: Option<PackClause> = args
                .get("tailored_clause")
                .cloned()
                .map(serde_json::from_value)
                .transpose()
                .map_err(|error| format!("invalid tailored clause: {error}"))?;
            ok(&engine
                .review_pack_clause(
                    req_str(args, "proposal_id")?,
                    disposition,
                    tailored.as_ref(),
                    req_str(args, "agent")?,
                    opt_str(args, "reason").as_deref(),
                )
                .map_err(|error| error.to_string())?)
        })()),
        "pack_clause_provenance" => Some((|| {
            ok(&engine
                .pack_clause_provenance(req_str(args, "goal_id")?)
                .map_err(|error| error.to_string())?)
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

#[cfg(test)]
mod tests {
    use super::super::{bind_session, call, list};
    use lodestar_core::{Lodestar, PackProposalBatch, PackReviewOutcome};
    use mindleak_session::SessionRegistry;

    use super::*;

    fn result_json(result: &Value) -> Value {
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    #[test]
    fn common_core_review_is_exposed_and_bound_to_the_registered_session() {
        let review = list()
            .into_iter()
            .find(|tool| tool["name"] == "review_pack_clause")
            .unwrap();
        assert!(review["inputSchema"]["properties"]["session_id"].is_object());
        assert!(review["inputSchema"]["properties"]["agent"].is_null());

        let engine = Lodestar::open_in_memory().unwrap();
        let proposed = call(
            &engine,
            &json!({ "name": "propose_common_core", "arguments": {} }),
        )
        .unwrap();
        let batch: PackProposalBatch = serde_json::from_value(result_json(&proposed)).unwrap();
        assert_eq!(batch.proposals.len(), 5);

        let sessions = SessionRegistry::new("reviewer").unwrap();
        let identity = sessions
            .open_session("00112233445566778899aabbccddeeff")
            .unwrap();
        let params = json!({
            "name": "review_pack_clause",
            "arguments": {
                "session_id": "00112233445566778899aabbccddeeff",
                "agent": "caller-spoof",
                "proposal_id": batch.proposals[0].id,
                "disposition": "adopted"
            }
        });
        let bound = bind_session(&params, &sessions).unwrap();
        assert_eq!(bound["arguments"]["agent"], identity.agent_id);
        let reviewed = call(&engine, &bound).unwrap();
        let outcome: PackReviewOutcome = serde_json::from_value(result_json(&reviewed)).unwrap();
        assert_eq!(
            outcome.proposal.reviewed_by.as_deref(),
            Some(identity.agent_id.as_str())
        );
        assert_eq!(outcome.goal.unwrap().origin.as_str(), "pack");
    }
}

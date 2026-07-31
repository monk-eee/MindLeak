//! Constitution tool definitions and dispatch.

use super::{constitution_packs as policy_packs, ok, opt_str, req_str, str_array, text};
use lodestar_core::{ArtifactBindingMode, GoalKind, Lodestar};
use serde_json::{json, Value};

pub(super) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "constitution_define",
            "description": "Write or rewrite constitutional intent (ADR-0059). `action` names the act: `goal` adds a durable objective, constraint or invariant; `supersede` replaces one with a new active version, retiring rather than deleting the old, which is the only way intent changes; `bind` and `unbind` attach and prune the artefacts a clause governs, which is what makes `touched_task_goal` answerable at all. Read the constitution before acting.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["goal", "supersede", "bind", "unbind"] },
                    "kind": { "type": "string", "enum": ["objective", "constraint", "invariant"], "description": "Required for goal." },
                    "title": { "type": "string", "description": "Required for goal." },
                    "statement": { "type": "string", "description": "Required for goal: the normative text, what must hold or be achieved." },
                    "parent_id": { "type": "string", "description": "Optional for goal: parent goal id for hierarchy." },
                    "goal_id": { "type": "string", "description": "Required for supersede, bind and unbind." },
                    "new_statement": { "type": "string", "description": "Required for supersede." },
                    "reason": { "type": "string", "description": "Required for supersede." },
                    "node_ids": { "type": "array", "items": { "type": "string" }, "description": "Required for bind and unbind." },
                    "mode": { "type": "string", "enum": ["governed", "forbid_change"], "description": "Optional for bind; defaults to governed." }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "constitution_decide",
            "description": "Move a constitution version through its lifecycle (ADR-0059). `action` names the transition: `propose` drafts a version for review; `activate` adopts a draft as the one in force. Adopting policy is an explicit act, so a draft that is not a draft, or one whose clauses are undecided, is refused rather than adopted quietly.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["propose", "activate"] },
                    "draft_id": { "type": "string", "description": "Required for activate." },
                    "version": { "type": "string", "description": "Required for propose." },
                    "activated_by": { "type": "string", "description": "Who adopted it; attributed." },
                    "agent": { "type": "string", "description": "Injected from the registered session." }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "constitution_query",
            "description": "Read the constitution and what it governs (ADR-0059). `action` names what: `active` returns the goals, constraints and invariants in force; `status` reports whether a constitution is adopted at all, which is how an agent tells 'no policy' from 'policy permits this'; `governing` returns the clauses governing one node; `for_task` returns those governing a task's goal; `export` renders it as committed-friendly markdown. Read-only and evidence-free.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["active", "status", "governing", "for_task", "export"] },
                    "node_id": { "type": "string", "description": "Required for governing." },
                    "task_id": { "type": "string", "description": "Required for for_task." },
                    "path": { "type": "string", "description": "Optional for export: write the markdown here." }
                },
                "required": ["action"]
            }
        }),
        json!({
            "name": "define_goal",
            "description": "Superseded by `constitution_define` (action: goal); still answered for one minor version so a session already in flight does not break. Add a durable constitution entry: an objective, constraint, or invariant that governs the work. Read the constitution before acting.",
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
            "description": "Superseded by `constitution_define` (action: supersede); still answered for one minor version. Replace a goal with a new active version (the old one is retired, not deleted). The only way intent changes — explicit and attributed.",
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
            "description": "Superseded by `constitution_query` (action: active); still answered for one minor version. Return the active goals, constraints, and invariants — the authoritative intent every agent should read before acting.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "link_goal_to_artifact",
            "description": "Link a goal to the MindLeak nodes (artifact:/symbol: ids) that realise it, so conformance can tell which intent governs a file. Bind whatever the goal actually delivers — source, but equally an ADR, documentation, a benchmark or a build script — so a task delivering it can answer for it instead of appearing to have touched nothing (ADR-0060). Documentation still never counts as drift against a goal that did not bind it.",
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
            "name": "unlink_goal_from_artifact",
            "description": "Remove goal↔artifact bindings — the inverse of link_goal_to_artifact. Prune a stale or mistaken binding (e.g. a shared doc, or a source file a goal no longer realises) so conformance stops flagging honest changes to it as drift against that goal. A node not bound to the goal is a no-op. Returns how many bindings were removed.",
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
            "description": "Audit which active goals govern a node, and how (governed / forbid_change) — inspect binding hygiene before pruning with unlink_goal_from_artifact.",
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
            "name": "constitution_status",
            "description": "Report whether this project has an active constitution, a draft awaiting review, or none at all, with the version and its clause count. Read-only; never proposes or activates.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "propose_constitution",
            "description": "Draft a constitution for an ungoverned project: classify the supplied repository paths into cited project facts, record them as the draft's provenance, and propose the Common Core for review. Deterministic, model-free, and never activates. Refuses an already-active constitution (that is an amendment) or an existing unresolved draft.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Workspace-relative repository paths to classify. The caller supplies them; the server performs no filesystem scan."
                    },
                    "session_id": { "type": "string", "pattern": "^[0-9a-f]{32}$", "description": "Session id previously registered with open_session." }
                },
                "required": ["paths", "session_id"]
            }
        }),
        json!({
            "name": "activate_constitution",
            "description": "Activate a reviewed draft as the governing constitution. One atomic transaction: refuses a draft with any undecided clause proposal, a draft with no clauses, anything that is not a draft, and activation while another version is already active. Adopted clauses are promoted with their version, so nothing governs until this succeeds.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "draft_id": { "type": "string", "description": "The drafted constitution version id, e.g. constitution:v1." },
                    "session_id": { "type": "string", "pattern": "^[0-9a-f]{32}$", "description": "Session id previously registered with open_session." }
                },
                "required": ["draft_id", "session_id"]
            }
        }),
    ]
    .into_iter()
    // The policy-pack half lives in its own module: two clusters share this
    // name, and keeping both here put the file past the module-length clause.
    .chain(policy_packs::definitions())
    .collect()
}

pub(super) fn dispatch(
    engine: &Lodestar,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    match name {
        // The collapsed vocabulary (ADR-0059). Each verb maps its `action` to
        // the implementation the superseded name already used, so there is one
        // code path rather than two that can drift — and every guard the old
        // name encoded survives untouched, now reached through an argument
        // instead of through a tool name. An unknown action is refused by
        // naming the valid ones, because a caller that guessed wrong has no
        // other way to discover the vocabulary.
        "constitution_define" => match req_str(args, "action") {
            Ok("goal") => dispatch(engine, "define_goal", args),
            Ok("supersede") => dispatch(engine, "supersede_goal", args),
            Ok("bind") => dispatch(engine, "link_goal_to_artifact", args),
            Ok("unbind") => dispatch(engine, "unlink_goal_from_artifact", args),
            Ok(other) => Some(Err(format!(
                "unknown action: {other}; constitution_define takes goal, supersede, bind or unbind"
            ))),
            Err(error) => Some(Err(error)),
        },
        "constitution_decide" => match req_str(args, "action") {
            Ok("propose") => dispatch(engine, "propose_constitution", args),
            Ok("activate") => dispatch(engine, "activate_constitution", args),
            Ok(other) => Some(Err(format!(
                "unknown action: {other}; constitution_decide takes propose or activate"
            ))),
            Err(error) => Some(Err(error)),
        },
        "constitution_query" => match req_str(args, "action") {
            Ok("active") => dispatch(engine, "get_constitution", args),
            Ok("status") => dispatch(engine, "constitution_status", args),
            Ok("governing") => dispatch(engine, "governing_goals", args),
            Ok("for_task") => dispatch(engine, "governing_for_task", args),
            Ok("export") => dispatch(engine, "export_constitution", args),
            Ok(other) => Some(Err(format!(
                "unknown action: {other}; constitution_query takes active, status, governing, for_task or export"
            ))),
            Err(error) => Some(Err(error)),
        },
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
        "link_goal_to_artifact" => Some((|| {
            let mode = parse_binding_mode(
                opt_str(args, "mode")
                    .unwrap_or_else(|| "governed".to_string())
                    .as_str(),
            )?;
            let linked = engine
                .link_goal_to_artifact(
                    req_str(args, "goal_id")?,
                    &str_array(args, "node_ids"),
                    mode,
                )
                .map_err(|e| e.to_string())?;
            ok(&json!({ "linked": linked }))
        })()),
        "unlink_goal_from_artifact" => Some((|| {
            let removed = engine
                .unlink_goal_from_artifact(req_str(args, "goal_id")?, &str_array(args, "node_ids"))
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
        "constitution_status" => Some((|| {
            ok(&engine
                .constitution_status()
                .map_err(|error| error.to_string())?)
        })()),
        // Actions are matched through `Ok(..)` rather than as bare string arms.
        // That is not style: `every_tool_the_server_answers_to_is_advertised`
        // scans this source for `"name" =>` to prove the server advertises
        // everything it answers to, and a bare inner arm reads to it as an
        // undeclared tool. Keeping the two shapes distinguishable keeps that
        // guard honest instead of teaching it to ignore things.
        "policy_pack_register" => match req_str(args, "action") {
            Ok("register") => Some(pack_register(engine, args)),
            Ok("propose") => Some(pack_propose(engine, args)),
            Ok("common_core") => Some(
                engine
                    .propose_common_core()
                    .map_err(|error| error.to_string())
                    .and_then(|value| ok(&value)),
            ),
            Ok("fleet_delivery") => Some(
                engine
                    .propose_fleet_delivery()
                    .map_err(|error| error.to_string())
                    .and_then(|value| ok(&value)),
            ),
            Ok(other) => Some(Err(format!(
                "unknown action: {other}; policy_pack_register takes register, propose, common_core, or fleet_delivery"
            ))),
            Err(error) => Some(Err(error)),
        },
        "policy_pack_decide" => match req_str(args, "action") {
            Ok("clause") => Some(pack_review_clause(engine, args)),
            Ok("contract") => Some(pack_complete_contract(engine, args)),
            Ok(other) => Some(Err(format!(
                "unknown action: {other}; policy_pack_decide takes clause or contract"
            ))),
            Err(error) => Some(Err(error)),
        },
        "policy_pack_query" => match req_str(args, "action") {
            Ok("proposals") => Some(pack_proposals(engine, args)),
            Ok("provenance") => Some(pack_provenance(engine, args)),
            Ok(other) => Some(Err(format!(
                "unknown action: {other}; policy_pack_query takes proposals or provenance"
            ))),
            Err(error) => Some(Err(error)),
        },
        "register_policy_pack" => Some(pack_register(engine, args)),
        "propose_policy_pack" => Some(pack_propose(engine, args)),
        "propose_common_core" => Some((|| {
            ok(&engine
                .propose_common_core()
                .map_err(|error| error.to_string())?)
        })()),
        "propose_fleet_delivery" => Some((|| {
            ok(&engine
                .propose_fleet_delivery()
                .map_err(|error| error.to_string())?)
        })()),
        "list_pack_proposals" => Some(pack_proposals(engine, args)),
        "review_pack_clause" => Some(pack_review_clause(engine, args)),
        "propose_constitution" => Some((|| {
            ok(&engine
                .propose_constitution(&str_array(args, "paths"), Some(req_str(args, "agent")?))
                .map_err(|error| error.to_string())?)
        })()),
        "activate_constitution" => Some((|| {
            ok(&engine
                .activate_constitution(req_str(args, "draft_id")?, req_str(args, "agent")?)
                .map_err(|error| error.to_string())?)
        })()),
        "pack_clause_provenance" => Some((|| {
            ok(&engine
                .pack_clause_provenance(req_str(args, "goal_id")?)
                .map_err(|error| error.to_string())?)
        })()),
        "complete_clause_contract" => Some(pack_complete_contract(engine, args)),
        _ => None,
    }
}

fn parse_kind(s: &str) -> Result<GoalKind, String> {
    GoalKind::from_tag(s).ok_or_else(|| format!("invalid kind: {s}"))
}

// One implementation per transition, called by both the collapsed verb and the
// name it replaced. ADR-0059 is explicit that a cluster is not collapsed until
// its guards move with it: every refusal a separate tool name used to encode
// becomes an argument validation carrying the same message, so the deprecated
// name and the new one cannot drift into answering differently.

fn pack_register(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    policy_packs::register(engine, args)
}

fn pack_propose(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    policy_packs::propose(engine, args)
}

fn pack_proposals(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    policy_packs::proposals(engine, args)
}

fn pack_provenance(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    policy_packs::provenance(engine, args)
}

fn pack_review_clause(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    policy_packs::review_clause(engine, args)
}

fn pack_complete_contract(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    policy_packs::complete_contract(engine, args)
}

fn parse_binding_mode(value: &str) -> Result<ArtifactBindingMode, String> {
    ArtifactBindingMode::from_tag(value)
        .ok_or_else(|| format!("invalid code binding mode: {value}"))
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
    fn constitution_status_reports_absent_on_a_fresh_engine() {
        // A project with no constitution must say so plainly, so an agent can
        // distinguish "no policy" from "policy permits this".
        assert!(list()
            .into_iter()
            .any(|tool| tool["name"] == "constitution_status"));

        let engine = Lodestar::open_in_memory().unwrap();
        let result = call(
            &engine,
            &json!({ "name": "constitution_status", "arguments": {} }),
        )
        .unwrap();
        let status = result_json(&result);
        assert_eq!(status["state"], "absent");
        assert!(status["version"].is_null());
        assert_eq!(status["clause_count"], 0);
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
            .open_session(
                "00112233445566778899aabbccddeeff",
                mindleak_session::SessionContext::default(),
            )
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

    // ADR-0059: the cluster collapses to a vocabulary. The transition matters
    // more than the count — a caller mid-session cannot read a changelog, so
    // the old names keep working for one minor version and answer with the call
    // to make.
    #[test]
    fn the_collapsed_verbs_are_advertised_alongside_the_names_they_replace() {
        let names: Vec<String> = list()
            .into_iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_string))
            .collect();

        for verb in [
            "constitution_define",
            "constitution_decide",
            "constitution_query",
        ] {
            assert!(names.iter().any(|n| n == verb), "{verb} is advertised");
        }

        // Still reachable, deliberately. Removing them in the same release that
        // introduced the replacements would break every caller in flight.
        for legacy in ["define_goal", "activate_constitution", "get_constitution"] {
            assert!(
                names.iter().any(|n| n == legacy),
                "{legacy} still answers during the deprecation window"
            );
        }
    }

    // A deprecation that does not teach is just a break with extra steps: the
    // old description has to name its replacement, because that string is the
    // only thing an agent mid-task will read.
    #[test]
    fn a_superseded_name_names_the_call_to_make_instead() {
        let legacy = list()
            .into_iter()
            .find(|tool| tool["name"] == "define_goal")
            .expect("define_goal is still advertised");
        let description = legacy["description"].as_str().unwrap_or_default();

        assert!(
            description.contains("constitution_define"),
            "the superseded tool must name its replacement: {description}"
        );
    }

    // The collapse must not lose a refusal. Every guard that a separate tool
    // name used to encode becomes an argument validation carrying the same
    // message — and adopting policy is an attributed act, so the guard worth
    // pinning is the one that refuses an unattributed activation. Reached now
    // through an argument rather than through a tool name, and unchanged.
    #[test]
    fn a_guard_survives_the_collapse_as_an_argument_validation() {
        let engine = Lodestar::open_in_memory().unwrap();

        let error = call(
            &engine,
            &json!({
                "name": "constitution_decide",
                "arguments": { "action": "activate", "draft_id": "constitution:absent" }
            }),
        )
        .unwrap_err();

        assert!(
            error.contains("agent"),
            "adopting policy stays attributed through the collapsed verb: {error}"
        );
    }

    // An unknown transition is refused by naming the ones that exist, because a
    // caller that guessed wrong has no other way to discover the vocabulary.
    #[test]
    fn an_unknown_transition_lists_the_ones_that_exist() {
        let engine = Lodestar::open_in_memory().unwrap();

        let error = call(
            &engine,
            &json!({
                "name": "constitution_decide",
                "arguments": { "action": "ratify", "draft_id": "constitution:v1" }
            }),
        )
        .unwrap_err();

        assert!(
            error.contains("activate"),
            "an unknown transition must name the valid ones: {error}"
        );
    }
}

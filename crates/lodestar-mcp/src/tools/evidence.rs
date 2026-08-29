//! Evidence export tool (ADR-0031): portable, verifiable proof-of-work.

use super::{opt_str, req_str, text};
use lodestar_core::Lodestar;
use serde_json::{json, Value};

pub(super) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "merge_evidence",
            "description": "Build an evidence bundle from a merge that already landed (ADR-0058), instead of assembling one by hand. Name the commit that carried this task's work; the plane verifies deterministically that git can resolve it, that it is reachable from main, and that it touched paths inside the task's declared scope, then derives the bundle from what git reports. It does NOT complete the task: conformance still judges the result and somebody still has to submit it. Pass the returned bundle to check_conformance and complete_task.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "commit": { "type": "string", "description": "The merge commit on main that carried this work, as git rev-parse reports it." },
                    "session_id": { "type": "string", "description": "Session id previously registered with open_session.", "pattern": "^[0-9a-f]{32}$" }
                },
                "required": ["task_id", "commit", "session_id"]
            }
        }),
        json!({
            "name": "ledger_act_evidence",
            "description": "Build an evidence bundle from one Lodestar-internal ledger act (ADR-0110) -- a design registration/decision, a granted waiver, a constitution amendment, or a goal supersession -- instead of refusing a ledger-only completion for having no MindLeak node mutation. The plane verifies deterministically, with no MindLeak call, that the named act exists, that ITS OWN recorded actor matches your resolved agent, and that its timestamp falls inside your live claim's window. It does NOT complete the task: conformance still judges the result and somebody still has to submit it. `kind` must be one of design_registered, design_decided, waiver_granted, constitution_amended, goal_superseded. For goal_superseded, `act_id` is the RETIRED clause's id; a clause superseded before ADR-0142 recorded no actor and is refused rather than attributed to you.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "kind": { "type": "string", "enum": ["design_registered", "design_decided", "waiver_granted", "constitution_amended", "goal_superseded"], "description": "Which closed ledger-act kind act_id names." },
                    "act_id": { "type": "string", "description": "The design item id, waiver id, amendment id, or retired goal id the act was recorded under." },
                    "session_id": { "type": "string", "description": "Session id previously registered with open_session.", "pattern": "^[0-9a-f]{32}$" }
                },
                "required": ["task_id", "kind", "act_id", "session_id"]
            }
        }),
        json!({
            "name": "export_evidence",
            "description": "Render a task's durable conformance evidence chain as committed-friendly, portable proof-of-work (ADR-0031): each check's stable id, verdict, acting agent, claim window, and evidence summary. Pass `path` to write the artifact (e.g. .lodestar/evidence/<task>.md) so the proof leaves the local ledger for review, CI, and audit. Deterministic and model-free.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "path": { "type": "string", "description": "Optional file path to write the artifact for review/CI." }
                },
                "required": ["task_id"]
            }
        }),
        json!({
            "name": "export_conformance_manifest",
            "description": "Render the repo-wide conformance manifest (ADR-0031): the governed code-node set plus per-task verdict and covered nodes — the machine-checkable artifact the CI conformance gate (scripts/conformance-gate.mjs) reads to fail merges that change governed code without an aligned receipt. Pass `path` to write it (e.g. .lodestar/evidence/manifest.json). Deterministic and model-free.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Optional file path to write the manifest JSON." }
                }
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
        "merge_evidence" => Some((|| {
            let evidence = engine
                .merge_evidence(
                    req_str(args, "task_id")?,
                    req_str(args, "commit")?,
                    // The agent bind_session resolved from the token, not the
                    // token itself: the facade compares this against the task's
                    // owner, which is a `session:v1:` id.
                    req_str(args, "agent")?,
                )
                .map_err(|e| e.to_string())?;
            text(serde_json::to_string_pretty(&evidence).map_err(|e| e.to_string())?)
        })()),
        "ledger_act_evidence" => Some((|| {
            let tag = req_str(args, "kind")?;
            let kind = lodestar_core::LedgerActKind::parse(tag).ok_or_else(|| {
                format!(
                    "unknown ledger-act kind {tag}; expected one of design_registered, \
                     design_decided, waiver_granted, constitution_amended"
                )
            })?;
            let evidence = engine
                .ledger_act_evidence(
                    req_str(args, "task_id")?,
                    kind,
                    req_str(args, "act_id")?,
                    req_str(args, "agent")?,
                )
                .map_err(|e| e.to_string())?;
            text(serde_json::to_string_pretty(&evidence).map_err(|e| e.to_string())?)
        })()),
        "export_evidence" => Some((|| {
            let markdown = engine
                .export_evidence(req_str(args, "task_id")?, opt_str(args, "path").as_deref())
                .map_err(|e| e.to_string())?;
            text(markdown)
        })()),
        "export_conformance_manifest" => Some((|| {
            let manifest = engine
                .export_conformance_manifest(opt_str(args, "path").as_deref())
                .map_err(|e| e.to_string())?;
            text(manifest)
        })()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    // Generated by AI (UnitTest MCP)
    use super::*;
    use lodestar_core::model::GoalKind;
    use lodestar_core::waiver::WaiverRequest;
    use lodestar_core::PackClauseDisposition;

    fn engine() -> Lodestar {
        Lodestar::open_in_memory().unwrap()
    }

    fn body(result: &Value) -> Value {
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap())
            .expect("ok()/text() emits valid JSON text")
    }

    fn task_for(e: &Lodestar, owner: &str) -> String {
        let goal = e
            .define_goal(GoalKind::Objective, "Evidence coverage", "ship it", None)
            .unwrap();
        let task = e.create_task(&goal.id, "Do the work", "done").unwrap();
        assert!(e.claim_task(&task.id, owner, 600).unwrap());
        task.id
    }

    /// A governed project with one already-active clause that explicitly
    /// declares itself waivable, mirroring `tools/waivers.rs`'s own test
    /// helper of the same shape: migration invents none of the enforcement
    /// fields for the Common Core, so plain `governed()` never produces a
    /// clause `grant_waiver` will accept.
    fn governed_with_waivable_clause(e: &Lodestar) -> String {
        let proposal = e
            .propose_constitution(&["README.md".to_string()], Some("monk-eee"))
            .unwrap();
        let draft_id = proposal.version.id.clone();
        for clause in proposal.common_core.proposals {
            e.review_pack_clause(
                &clause.id,
                PackClauseDisposition::Adopted,
                None,
                "monk-eee",
                Some("Adopted as proposed"),
            )
            .unwrap();
        }
        let clause = e
            .draft_clause(
                &draft_id,
                GoalKind::Constraint,
                "Waivable rule",
                "Something must hold, but an exception may be granted.",
            )
            .unwrap();
        e.complete_clause_contract(&clause.id, "artifact:crates/**", "tests", None, true, None)
            .unwrap();
        e.activate_constitution(&draft_id, "monk-eee").unwrap();
        clause.id
    }

    #[test]
    fn unknown_tools_are_not_claimed_by_this_module() {
        assert!(dispatch(&engine(), "grant_waiver", &json!({})).is_none());
    }

    #[test]
    fn merge_evidence_dispatch_reports_each_missing_required_argument() {
        let engine = engine();
        assert_eq!(
            dispatch(&engine, "merge_evidence", &json!({}))
                .unwrap()
                .unwrap_err(),
            "missing required string arg: task_id"
        );
        assert_eq!(
            dispatch(&engine, "merge_evidence", &json!({ "task_id": "task:x" }))
                .unwrap()
                .unwrap_err(),
            "missing required string arg: commit"
        );
        assert_eq!(
            dispatch(
                &engine,
                "merge_evidence",
                &json!({ "task_id": "task:x", "commit": "abc" })
            )
            .unwrap()
            .unwrap_err(),
            "missing required string arg: agent"
        );
    }

    #[test]
    fn merge_evidence_dispatch_refuses_without_a_configured_workspace_root() {
        let engine = engine();
        let err = dispatch(
            &engine,
            "merge_evidence",
            &json!({ "task_id": "task:x", "commit": "abc123", "agent": "agent-a" }),
        )
        .expect("tool is dispatched")
        .expect_err("open_in_memory() never configures a workspace root");
        assert_eq!(
            err,
            "invalid: this server was not told which checkout it serves, so it cannot verify a \
             merge; set MINDLEAK_WORKSPACE or complete with an evidence bundle instead"
        );
    }

    #[test]
    fn ledger_act_evidence_dispatch_reports_each_missing_required_argument() {
        let engine = engine();
        assert_eq!(
            dispatch(&engine, "ledger_act_evidence", &json!({}))
                .unwrap()
                .unwrap_err(),
            "missing required string arg: kind"
        );
        assert_eq!(
            dispatch(
                &engine,
                "ledger_act_evidence",
                &json!({ "kind": "waiver_granted" })
            )
            .unwrap()
            .unwrap_err(),
            "missing required string arg: task_id"
        );
        assert_eq!(
            dispatch(
                &engine,
                "ledger_act_evidence",
                &json!({ "kind": "waiver_granted", "task_id": "task:x" })
            )
            .unwrap()
            .unwrap_err(),
            "missing required string arg: act_id"
        );
        assert_eq!(
            dispatch(
                &engine,
                "ledger_act_evidence",
                &json!({ "kind": "waiver_granted", "task_id": "task:x", "act_id": "waiver:x" })
            )
            .unwrap()
            .unwrap_err(),
            "missing required string arg: agent"
        );
    }

    #[test]
    fn ledger_act_evidence_dispatch_rejects_an_unknown_kind() {
        let engine = engine();
        let err = dispatch(
            &engine,
            "ledger_act_evidence",
            &json!({
                "task_id": "task:x",
                "kind": "not-a-real-kind",
                "act_id": "x",
                "agent": "agent-a",
            }),
        )
        .expect("tool is dispatched")
        .expect_err("not-a-real-kind is not a LedgerActKind");
        assert_eq!(
            err,
            "unknown ledger-act kind not-a-real-kind; expected one of design_registered, \
             design_decided, waiver_granted, constitution_amended"
        );
    }

    #[test]
    fn ledger_act_evidence_dispatch_reports_not_found_for_an_unknown_task() {
        let engine = engine();
        let err = dispatch(
            &engine,
            "ledger_act_evidence",
            &json!({
                "task_id": "task:doesnotexist",
                "kind": "waiver_granted",
                "act_id": "waiver:x",
                "agent": "agent-a",
            }),
        )
        .expect("tool is dispatched")
        .expect_err("no such task was ever created");
        assert_eq!(err, "not found: task:doesnotexist");
    }

    #[test]
    fn ledger_act_evidence_dispatch_refuses_an_agent_who_does_not_hold_the_task() {
        let engine = engine();
        let task_id = task_for(&engine, "agent-owner");

        let err = dispatch(
            &engine,
            "ledger_act_evidence",
            &json!({
                "task_id": task_id,
                "kind": "waiver_granted",
                "act_id": "waiver:x",
                "agent": "agent-other",
            }),
        )
        .expect("tool is dispatched")
        .expect_err("agent-other never claimed this task");
        assert!(err.contains("is not held by agent-other"), "{err}");
    }

    #[test]
    fn ledger_act_evidence_dispatch_builds_a_bundle_from_a_granted_waiver() {
        let engine = engine();
        let clause_id = governed_with_waivable_clause(&engine);
        let task_id = task_for(&engine, "agent-a");
        let granted = engine
            .grant_waiver(&WaiverRequest {
                clause_id,
                scope: "artifact:crates/**".to_string(),
                reason: "testing".to_string(),
                approved_by: "agent-a".to_string(),
                expires_at: 4_000_000_000,
                remediation_task_id: None,
            })
            .unwrap();

        let result = dispatch(
            &engine,
            "ledger_act_evidence",
            &json!({
                "task_id": task_id,
                "kind": "waiver_granted",
                "act_id": granted.id,
                "agent": "agent-a",
            }),
        )
        .expect("tool is dispatched")
        .expect("a waiver granted by the task's own owner is valid evidence");
        let bundle = body(&result);
        assert_eq!(bundle["task_id"], task_id);
        assert_eq!(bundle["agent_id"], "agent-a");
        assert_eq!(bundle["changed_node_ids"].as_array().unwrap().len(), 0);
        assert_eq!(bundle["commit_ids"].as_array().unwrap().len(), 0);
        assert_eq!(
            bundle["ledger_act_ids"][0],
            format!("ledger_act:waiver_granted:{}", granted.id)
        );
    }

    #[test]
    fn export_evidence_dispatch_reports_missing_task_id() {
        assert_eq!(
            dispatch(&engine(), "export_evidence", &json!({}))
                .unwrap()
                .unwrap_err(),
            "missing required string arg: task_id"
        );
    }

    #[test]
    fn export_evidence_dispatch_renders_a_task_with_no_conformance_records() {
        let engine = engine();
        let task_id = task_for(&engine, "agent-a");
        let result = dispatch(&engine, "export_evidence", &json!({ "task_id": task_id }))
            .expect("tool is dispatched")
            .expect("a task always has an evidence chain, even an empty one");
        let markdown = result["content"][0]["text"].as_str().unwrap();
        assert_eq!(
            markdown,
            format!(
                "# Conformance evidence for `{task_id}`\n\n> Generated by Lodestar (ADR-0031). \
                 Portable proof-of-work: each row resolves to a durable, provenance-bearing \
                 evidence record. Do not edit by hand.\n\n_No conformance records: this task \
                 never reached a checked completion._\n"
            )
        );
    }

    #[test]
    fn export_conformance_manifest_dispatch_renders_an_empty_manifest_on_a_fresh_engine() {
        let result = dispatch(&engine(), "export_conformance_manifest", &json!({}))
            .expect("tool is dispatched")
            .expect("the manifest read never fails");
        let manifest: Value =
            serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(manifest["schema"], 1);
        assert_eq!(manifest["governed_nodes"].as_array().unwrap().len(), 0);
        assert_eq!(manifest["receipts"].as_array().unwrap().len(), 0);
    }
}

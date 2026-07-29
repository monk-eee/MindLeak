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

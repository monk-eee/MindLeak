//! Evidence export tool (ADR-0031): portable, verifiable proof-of-work.

use super::{opt_str, req_str, text};
use lodestar_core::Lodestar;
use serde_json::{json, Value};

pub(super) fn definitions() -> Vec<Value> {
    vec![
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

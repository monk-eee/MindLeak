//! Typed control tool definitions and dispatch (ADR-0034, SPEC-CONSTITUTION §4).

use super::{f64_arg, ok, req_str, str_array};
use lodestar_core::controls::{Ratchet, RatchetDirection};
use lodestar_core::Lodestar;
use serde_json::{json, Value};

pub(super) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "register_ratchet",
            "description": "Bind a ratchet to one constitutional clause: a metric that must not regress past a reviewed baseline. A ratchet reports; it never decides — its observed power caps it at review, because whether a given regression is acceptable is a judgement about the change. Registered without a baseline it reports unknown until one is accepted.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "control_id": { "type": "string", "description": "Stable id, e.g. control:coverage." },
                    "clause_id": { "type": "string", "description": "The active clause that authorises this ratchet." },
                    "metric": { "type": "string", "description": "What is measured, e.g. coverage.line_pct." },
                    "direction": {
                        "type": "string",
                        "enum": ["non_decreasing", "non_increasing"],
                        "description": "non_decreasing for coverage or pass rate; non_increasing for warning counts or latency."
                    },
                    "tolerance": { "type": "number", "default": 0.0, "description": "Movement no larger than this is noise, not a regression." }
                },
                "required": ["control_id", "clause_id", "metric", "direction"]
            }
        }),
        json!({
            "name": "accept_ratchet_baseline",
            "description": "Accept the value a ratchet compares against, attributed to the calling session. A ratchet never moves its own baseline: a mechanism that adopts whatever it last measured launders a regression into the new normal. Bumps the control version, so observations taken against the old baseline resolve as unknown rather than being re-judged against a number they never saw.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "control_id": { "type": "string" },
                    "value": { "type": "number" }
                },
                "required": ["control_id", "value"]
            }
        }),
        json!({
            "name": "observe_ratchet",
            "description": "Report one measurement to a registered ratchet and resolve it through its clause. The baseline is read from the store, never supplied by the caller. Returns pass/fail/unknown plus the effective consequence after the ADR-0034 enforcement ceiling.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "control_id": { "type": "string" },
                    "measured": { "type": "number" },
                    "scope": { "type": "string", "description": "What was measured over, e.g. an artifact: id or a workflow: scope." },
                    "evidence_refs": { "type": "array", "items": { "type": "string" }, "description": "Ids of the runs or reports the measurement came from." }
                },
                "required": ["control_id", "measured", "scope"]
            }
        }),
        json!({
            "name": "clause_controls",
            "description": "List the controls bound to one clause, with the enforcement power each actually has — the mechanisms behind a rule, and therefore the hardest consequence it can reach.",
            "inputSchema": {
                "type": "object",
                "properties": { "clause_id": { "type": "string" } },
                "required": ["clause_id"]
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
        "register_ratchet" => Some(register_ratchet(engine, args)),
        "accept_ratchet_baseline" => Some(accept_ratchet_baseline(engine, args)),
        "observe_ratchet" => Some(observe_ratchet(engine, args)),
        "clause_controls" => Some(clause_controls(engine, args)),
        _ => None,
    }
}

fn register_ratchet(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let direction = match req_str(args, "direction")? {
        "non_decreasing" => RatchetDirection::NonDecreasing,
        "non_increasing" => RatchetDirection::NonIncreasing,
        other => return Err(format!("unknown ratchet direction: {other}")),
    };
    let ratchet = Ratchet::new(
        req_str(args, "control_id")?,
        req_str(args, "clause_id")?,
        req_str(args, "metric")?,
        direction,
        f64_arg(args, "tolerance", 0.0),
    )
    .map_err(|e| e.to_string())?;
    let control = engine
        .register_ratchet(&ratchet)
        .map_err(|e| e.to_string())?;
    ok(&json!({
        "control_id": control.id,
        "clause_id": control.clause_id,
        "power": control.power.as_str(),
        "version": control.version,
        "baseline": Value::Null,
        "note": "no reviewed baseline yet; this ratchet reports unknown until one is accepted",
    }))
}

fn accept_ratchet_baseline(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let reviewed = engine
        .accept_ratchet_baseline(
            req_str(args, "control_id")?,
            args.get("value")
                .and_then(Value::as_f64)
                .ok_or_else(|| "missing required number arg: value".to_string())?,
            req_str(args, "agent")?,
        )
        .map_err(|e| e.to_string())?;
    let baseline = reviewed
        .baseline
        .as_ref()
        .ok_or_else(|| "baseline was not recorded".to_string())?;
    ok(&json!({
        "control_id": reviewed.control_id,
        "metric": reviewed.metric,
        "baseline": baseline.value,
        "reviewed_by": baseline.reviewed_by,
        "version": reviewed.version,
    }))
}

fn observe_ratchet(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let resolution = engine
        .observe_ratchet(
            req_str(args, "control_id")?,
            args.get("measured")
                .and_then(Value::as_f64)
                .ok_or_else(|| "missing required number arg: measured".to_string())?,
            req_str(args, "scope")?,
            str_array(args, "evidence_refs"),
        )
        .map_err(|e| e.to_string())?;
    ok(&json!({
        "control_id": resolution.control_id,
        "clause_id": resolution.clause_id,
        "status": resolution.status,
        "effective": resolution.effective.as_str(),
        "finding": resolution.finding,
    }))
}

fn clause_controls(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let controls = engine
        .clause_controls(req_str(args, "clause_id")?)
        .map_err(|e| e.to_string())?;
    let rows: Vec<Value> = controls
        .iter()
        .map(|control| {
            json!({
                "control_id": control.id,
                "kind": control.kind,
                "power": control.power.as_str(),
                "ceiling": control.power.ceiling().as_str(),
                "version": control.version,
                "status": control.status,
            })
        })
        .collect();
    ok(&json!({ "controls": rows }))
}

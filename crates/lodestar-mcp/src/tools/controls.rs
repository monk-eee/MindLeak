//! Typed control tool definitions and dispatch (ADR-0034, SPEC-CONSTITUTION §4).

use super::{f64_arg, i64_arg, ok, opt_str, req_str, str_array};
use lodestar_core::controls::{
    Control, ControlKind, ControlStatus, EnforcementPower, ObservationStatus, Ratchet,
    RatchetDirection,
};
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
            "description": "Accept the value a ratchet compares against, attributed to the calling session and justified by a required reason. A ratchet never moves its own baseline: a mechanism that adopts whatever it last measured launders a regression into the new normal. The reason is required because who moved the number does not say whether moving it was justified, which is the question the mechanism cannot answer about itself. Bumps the control version, so observations taken against the old baseline resolve as unknown rather than being re-judged against a number they never saw.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "control_id": { "type": "string" },
                    "value": { "type": "number" },
                    "reason": {
                        "type": "string",
                        "description": "Why this baseline is trustworthy — the judgement the ratchet cannot make about itself."
                    }
                },
                "required": ["control_id", "value", "reason"]
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
        json!({
            "name": "register_control",
            "description": "Bind a versioned mechanism to a clause. Without one a clause is an orphan: it resolves at advise no matter what consequence it declares, because a rule with no mechanism behind it is a preference (ADR-0034). Declare the power the mechanism honestly has — mechanical only if it genuinely prevented the action (a hook exited non-zero, a required gate went red); observed if it proves after the fact from complete data; advisory if it reports a hint that may be stale. Observed and advisory cap at review, which is the point: an advisory that looks like a mutex grants false safety.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "control_id": { "type": "string", "description": "Stable id, e.g. control:branch-policy." },
                    "clause_id": { "type": "string" },
                    "kind": { "type": "string", "enum": ["check", "threshold", "ratchet", "procedure", "judgment"] },
                    "power": { "type": "string", "enum": ["mechanical", "observed", "advisory"] },
                    "version": { "type": "integer", "default": 1, "description": "Bump when what the control does changes; a version never moves backwards, and a mismatched observation resolves as unknown rather than being trusted." },
                    "configuration": { "type": "string", "description": "Optional opaque configuration for the mechanism." }
                },
                "required": ["control_id", "clause_id", "kind", "power"]
            }
        }),
        json!({
            "name": "retire_control",
            "description": "Stand a control down when it is superseded, misregistered, or no longer the mechanism behind its clause. Attributed to the calling session and permanent: retiring a control is the one act that reduces what a clause can enforce without changing a word of the clause, so it is recorded like a waiver. Retirement is not deletion - the control stays, so observations naming it resolve as unknown rather than silently disappearing, and it keeps recording what it once enforced. Without this a control registered under the wrong id cannot be withdrawn at all, because a control version never moves backwards, so dead and duplicate mechanisms accumulate against live clauses and go on reporting.",
            "inputSchema": {
                "type": "object",
                "properties": { "control_id": { "type": "string" } },
                "required": ["control_id"]
            }
        }),
        json!({
            "name": "observe_control",
            "description": "Report one pass/fail/unknown observation to any registered control and resolve it through its clause (ADR-0034 ceiling applies). The generic counterpart to observe_ratchet: use this for a plain check/threshold/procedure/judgment control that has no baseline to read -- the caller's own deterministic classification is the whole observation. An unregistered control_id is refused outright rather than silently resolved as an orphan, so a typo in control_id fails loudly instead of quietly reporting nothing.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "control_id": { "type": "string" },
                    "status": { "type": "string", "enum": ["pass", "fail", "unknown"] },
                    "scope": { "type": "string", "description": "What was checked, e.g. a tool_invocation: node id." },
                    "evidence_refs": { "type": "array", "items": { "type": "string" }, "default": [] },
                    "measurements": { "type": "string", "description": "Optional free-form detail about what was found." }
                },
                "required": ["control_id", "status", "scope"]
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
        "register_control" => Some(register_control(engine, args)),
        "retire_control" => Some(retire_control(engine, args)),
        "observe_control" => Some(observe_control(engine, args)),
        _ => None,
    }
}

fn register_control(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let kind = match req_str(args, "kind")? {
        "check" => ControlKind::Check,
        "threshold" => ControlKind::Threshold,
        "ratchet" => ControlKind::Ratchet,
        "procedure" => ControlKind::Procedure,
        "judgment" => ControlKind::Judgment,
        other => return Err(format!("unknown control kind: {other}")),
    };
    let power_tag = req_str(args, "power")?;
    let power = EnforcementPower::from_tag(power_tag)
        .ok_or_else(|| format!("unknown enforcement power: {power_tag}"))?;
    let control = engine
        .register_control(&Control {
            id: req_str(args, "control_id")?.to_string(),
            clause_id: req_str(args, "clause_id")?.to_string(),
            kind,
            power,
            version: i64_arg(args, "version", 1),
            configuration: opt_str(args, "configuration"),
            status: ControlStatus::Active,
            retired_by: None,
            retired_at: None,
        })
        .map_err(|e| e.to_string())?;
    ok(&json!({
        "control_id": control.id,
        "clause_id": control.clause_id,
        "power": control.power.as_str(),
        "ceiling": control.power.ceiling().as_str(),
        "version": control.version,
    }))
}

fn retire_control(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let control_id = req_str(args, "control_id")?;
    let retired_by = req_str(args, "agent")?;
    let retired = engine
        .retire_control(control_id, retired_by)
        .map_err(|e| e.to_string())?;
    if !retired {
        return Err(format!(
            "no control {control_id} to retire; a control that was never registered cannot be stood down"
        ));
    }
    ok(&json!({
        "control_id": control_id,
        "status": "retired",
        "retired_by": retired_by
    }))
}

fn observe_control(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let status = match req_str(args, "status")? {
        "pass" => ObservationStatus::Pass,
        "fail" => ObservationStatus::Fail,
        "unknown" => ObservationStatus::Unknown,
        other => return Err(format!("unknown observation status: {other}")),
    };
    let resolution = engine
        .observe_control(
            req_str(args, "control_id")?,
            status,
            req_str(args, "scope")?,
            str_array(args, "evidence_refs"),
            opt_str(args, "measurements"),
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
            req_str(args, "reason")?,
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
        "reason": baseline.reason,
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

#[cfg(test)]
mod tests {
    // Generated by AI (UnitTest MCP)
    use super::*;

    fn engine() -> Lodestar {
        Lodestar::open_in_memory().unwrap()
    }

    fn body(result: &Value) -> Value {
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap())
            .expect("ok() emits valid JSON text")
    }

    /// A real active clause -- register_control validates the clause_id
    /// against the goals table, unlike register_ratchet, which deliberately
    /// allows an orphan control to report its own "serves no active clause"
    /// finding.
    fn real_clause(e: &Lodestar) -> String {
        e.define_goal(
            lodestar_core::model::GoalKind::Constraint,
            "Branch policy",
            "Every merge to main must pass required checks.",
            None,
        )
        .unwrap()
        .id
    }

    #[test]
    fn unknown_tools_are_not_claimed_by_this_module() {
        assert!(dispatch(&engine(), "grant_waiver", &json!({})).is_none());
    }

    #[test]
    fn register_control_dispatch_rejects_an_unknown_kind() {
        let err = dispatch(
            &engine(),
            "register_control",
            &json!({
                "control_id": "control:x",
                "clause_id": "clause:x",
                "kind": "not-a-real-kind",
                "power": "mechanical",
            }),
        )
        .expect("tool is dispatched")
        .expect_err("not-a-real-kind is not a ControlKind");
        assert_eq!(err, "unknown control kind: not-a-real-kind");
    }

    #[test]
    fn register_control_dispatch_rejects_an_unknown_power() {
        let err = dispatch(
            &engine(),
            "register_control",
            &json!({
                "control_id": "control:x",
                "clause_id": "clause:x",
                "kind": "check",
                "power": "not-a-real-power",
            }),
        )
        .expect("tool is dispatched")
        .expect_err("not-a-real-power is not an EnforcementPower");
        assert_eq!(err, "unknown enforcement power: not-a-real-power");
    }

    #[test]
    fn register_control_dispatch_reports_not_found_for_an_unregistered_clause() {
        let err = dispatch(
            &engine(),
            "register_control",
            &json!({
                "control_id": "control:hook",
                "clause_id": "clause:ghost",
                "kind": "check",
                "power": "mechanical",
            }),
        )
        .expect("tool is dispatched")
        .expect_err("clause:ghost was never defined as a goal");
        assert_eq!(
            err,
            "not found: clause clause:ghost for control control:hook"
        );
    }

    #[test]
    fn register_control_dispatch_registers_and_reports_the_ceiling() {
        let engine = engine();
        let clause_id = real_clause(&engine);
        let result = dispatch(
            &engine,
            "register_control",
            &json!({
                "control_id": "control:hook",
                "clause_id": clause_id,
                "kind": "check",
                "power": "mechanical",
            }),
        )
        .expect("tool is dispatched")
        .expect("a valid kind and power register cleanly against a real clause");
        let registered = body(&result);
        assert_eq!(registered["control_id"], "control:hook");
        assert_eq!(registered["clause_id"], clause_id);
        assert_eq!(registered["power"], "mechanical");
        assert_eq!(registered["ceiling"], "block");
        assert_eq!(registered["version"], 1);
    }

    #[test]
    fn retire_control_dispatch_reports_each_missing_required_argument() {
        let engine = engine();
        assert_eq!(
            dispatch(&engine, "retire_control", &json!({}))
                .unwrap()
                .unwrap_err(),
            "missing required string arg: control_id"
        );
        assert_eq!(
            dispatch(
                &engine,
                "retire_control",
                &json!({ "control_id": "control:x" })
            )
            .unwrap()
            .unwrap_err(),
            "missing required string arg: agent"
        );
    }

    #[test]
    fn retire_control_dispatch_refuses_a_control_that_was_never_registered() {
        let err = dispatch(
            &engine(),
            "retire_control",
            &json!({ "control_id": "control:ghost", "agent": "agent-a" }),
        )
        .expect("tool is dispatched")
        .expect_err("no such control was ever registered");
        assert_eq!(
            err,
            "no control control:ghost to retire; a control that was never registered cannot be stood down"
        );
    }

    #[test]
    fn retire_control_dispatch_stands_down_a_registered_control() {
        let engine = engine();
        let clause_id = real_clause(&engine);
        dispatch(
            &engine,
            "register_control",
            &json!({
                "control_id": "control:hook",
                "clause_id": clause_id,
                "kind": "check",
                "power": "mechanical",
            }),
        )
        .unwrap()
        .unwrap();

        let result = dispatch(
            &engine,
            "retire_control",
            &json!({ "control_id": "control:hook", "agent": "agent-a" }),
        )
        .expect("tool is dispatched")
        .expect("a registered control can be retired");
        let retired = body(&result);
        assert_eq!(retired["control_id"], "control:hook");
        assert_eq!(retired["status"], "retired");
        assert_eq!(retired["retired_by"], "agent-a");
    }

    #[test]
    fn register_ratchet_dispatch_rejects_an_unknown_direction() {
        let err = dispatch(
            &engine(),
            "register_ratchet",
            &json!({
                "control_id": "control:coverage",
                "clause_id": "clause:x",
                "metric": "coverage.line_pct",
                "direction": "not-a-real-direction",
            }),
        )
        .expect("tool is dispatched")
        .expect_err("not-a-real-direction is not a RatchetDirection");
        assert_eq!(err, "unknown ratchet direction: not-a-real-direction");
    }

    #[test]
    fn register_ratchet_dispatch_rejects_a_negative_tolerance() {
        let err = dispatch(
            &engine(),
            "register_ratchet",
            &json!({
                "control_id": "control:coverage",
                "clause_id": "clause:x",
                "metric": "coverage.line_pct",
                "direction": "non_decreasing",
                "tolerance": -1.0,
            }),
        )
        .expect("tool is dispatched")
        .expect_err("a negative tolerance is invalid");
        assert_eq!(err, "invalid: ratchet tolerance must not be negative");
    }

    #[test]
    fn register_ratchet_dispatch_registers_with_no_baseline_yet() {
        let engine = engine();
        let clause_id = real_clause(&engine);
        let result = dispatch(
            &engine,
            "register_ratchet",
            &json!({
                "control_id": "control:coverage",
                "clause_id": clause_id,
                "metric": "coverage.line_pct",
                "direction": "non_decreasing",
            }),
        )
        .expect("tool is dispatched")
        .expect("a valid ratchet registers cleanly against a real clause");
        let registered = body(&result);
        assert_eq!(registered["control_id"], "control:coverage");
        assert_eq!(registered["clause_id"], clause_id);
        assert_eq!(registered["power"], "observed");
        assert_eq!(registered["version"], 1);
        assert!(registered["baseline"].is_null());
        assert_eq!(
            registered["note"],
            "no reviewed baseline yet; this ratchet reports unknown until one is accepted"
        );
    }

    #[test]
    fn accept_ratchet_baseline_dispatch_reports_missing_value() {
        assert_eq!(
            dispatch(
                &engine(),
                "accept_ratchet_baseline",
                &json!({ "control_id": "control:coverage", "reason": "why" })
            )
            .unwrap()
            .unwrap_err(),
            "missing required number arg: value"
        );
    }

    #[test]
    fn accept_ratchet_baseline_dispatch_reports_not_found_for_an_unregistered_ratchet() {
        let err = dispatch(
            &engine(),
            "accept_ratchet_baseline",
            &json!({
                "control_id": "control:ghost",
                "value": 80.0,
                "agent": "agent-a",
                "reason": "why",
            }),
        )
        .expect("tool is dispatched")
        .expect_err("no ratchet was ever registered under this id");
        assert_eq!(err, "not found: control:ghost");
    }

    #[test]
    fn accept_ratchet_baseline_dispatch_accepts_an_attributed_baseline() {
        let engine = engine();
        let clause_id = real_clause(&engine);
        dispatch(
            &engine,
            "register_ratchet",
            &json!({
                "control_id": "control:coverage",
                "clause_id": clause_id,
                "metric": "coverage.line_pct",
                "direction": "non_decreasing",
            }),
        )
        .unwrap()
        .unwrap();

        let result = dispatch(
            &engine,
            "accept_ratchet_baseline",
            &json!({
                "control_id": "control:coverage",
                "value": 90.38,
                "agent": "agent-a",
                "reason": "Measured from the current workspace-wide llvm-cov run.",
            }),
        )
        .expect("tool is dispatched")
        .expect("a registered ratchet accepts a reasoned baseline");
        let accepted = body(&result);
        assert_eq!(accepted["control_id"], "control:coverage");
        assert_eq!(accepted["metric"], "coverage.line_pct");
        assert_eq!(accepted["baseline"], 90.38);
        assert_eq!(accepted["reviewed_by"], "agent-a");
        assert_eq!(
            accepted["reason"],
            "Measured from the current workspace-wide llvm-cov run."
        );
        assert_eq!(accepted["version"], 2);
    }

    #[test]
    fn observe_ratchet_dispatch_reports_missing_measured() {
        assert_eq!(
            dispatch(
                &engine(),
                "observe_ratchet",
                &json!({ "control_id": "control:coverage", "scope": "artifact:x" })
            )
            .unwrap()
            .unwrap_err(),
            "missing required number arg: measured"
        );
    }

    #[test]
    fn observe_ratchet_dispatch_reports_not_found_for_an_unregistered_ratchet() {
        let err = dispatch(
            &engine(),
            "observe_ratchet",
            &json!({ "control_id": "control:ghost", "measured": 80.0, "scope": "artifact:x" }),
        )
        .expect("tool is dispatched")
        .expect_err("no ratchet was ever registered under this id");
        assert_eq!(err, "not found: control:ghost");
    }

    #[test]
    fn observe_ratchet_dispatch_reports_unknown_without_a_reviewed_baseline() {
        let engine = engine();
        let clause_id = real_clause(&engine);
        dispatch(
            &engine,
            "register_ratchet",
            &json!({
                "control_id": "control:coverage",
                "clause_id": clause_id,
                "metric": "coverage.line_pct",
                "direction": "non_decreasing",
            }),
        )
        .unwrap()
        .unwrap();

        let result = dispatch(
            &engine,
            "observe_ratchet",
            &json!({
                "control_id": "control:coverage",
                "measured": 91.0,
                "scope": "artifact:crates/lib.rs",
            }),
        )
        .expect("tool is dispatched")
        .expect("an unbaselined ratchet still resolves, just as unknown");
        let resolution = body(&result);
        assert_eq!(resolution["control_id"], "control:coverage");
        assert_eq!(resolution["clause_id"], clause_id);
        assert_eq!(resolution["status"], "unknown");
        assert_eq!(resolution["effective"], "advise");
        assert_eq!(
            resolution["finding"],
            "control control:coverage could not determine an answer; absence of evidence is not conformance"
        );
    }

    #[test]
    fn observe_ratchet_dispatch_reports_a_baselined_pass() {
        let engine = engine();
        let clause_id = real_clause(&engine);
        dispatch(
            &engine,
            "register_ratchet",
            &json!({
                "control_id": "control:coverage",
                "clause_id": clause_id,
                "metric": "coverage.line_pct",
                "direction": "non_decreasing",
            }),
        )
        .unwrap()
        .unwrap();
        dispatch(
            &engine,
            "accept_ratchet_baseline",
            &json!({
                "control_id": "control:coverage",
                "value": 90.0,
                "agent": "agent-a",
                "reason": "baseline",
            }),
        )
        .unwrap()
        .unwrap();

        let result = dispatch(
            &engine,
            "observe_ratchet",
            &json!({
                "control_id": "control:coverage",
                "measured": 91.0,
                "scope": "artifact:crates/lib.rs",
            }),
        )
        .expect("tool is dispatched")
        .expect("a measurement at or above baseline passes");
        let resolution = body(&result);
        assert_eq!(resolution["status"], "pass");
        assert_eq!(resolution["effective"], "advise");
    }

    #[test]
    fn clause_controls_dispatch_reports_missing_clause_id() {
        assert_eq!(
            dispatch(&engine(), "clause_controls", &json!({}))
                .unwrap()
                .unwrap_err(),
            "missing required string arg: clause_id"
        );
    }

    #[test]
    fn clause_controls_dispatch_reports_nothing_for_an_unbound_clause() {
        let result = dispatch(
            &engine(),
            "clause_controls",
            &json!({ "clause_id": "clause:nobody" }),
        )
        .expect("tool is dispatched")
        .expect("a clause with no controls is not an error");
        assert_eq!(body(&result)["controls"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn clause_controls_dispatch_lists_the_bound_control() {
        let engine = engine();
        let clause_id = real_clause(&engine);
        dispatch(
            &engine,
            "register_control",
            &json!({
                "control_id": "control:hook",
                "clause_id": clause_id,
                "kind": "check",
                "power": "mechanical",
            }),
        )
        .unwrap()
        .unwrap();

        let result = dispatch(
            &engine,
            "clause_controls",
            &json!({ "clause_id": clause_id }),
        )
        .expect("tool is dispatched")
        .expect("the read never fails");
        let controls = body(&result)["controls"].as_array().unwrap().clone();
        assert_eq!(controls.len(), 1);
        assert_eq!(controls[0]["control_id"], "control:hook");
        assert_eq!(controls[0]["kind"], "check");
        assert_eq!(controls[0]["power"], "mechanical");
        assert_eq!(controls[0]["ceiling"], "block");
        assert_eq!(controls[0]["version"], 1);
        assert_eq!(controls[0]["status"], "active");
    }
}

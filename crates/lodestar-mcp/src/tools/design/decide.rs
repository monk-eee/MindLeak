//! `design_decide`: every attributed human act on a design (accept, reject,
//! defer, resume, retire, supersede, reopen, attribute).

use lodestar_core::{DesignActionKind, Lodestar};
use serde_json::{json, Value};

use super::super::{ok, one_of, opt_str, required_for};
use super::constants::{BATCH_DECISIONS, DECISIONS};

pub(super) fn decide(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let decision = one_of(args, "decision", &DECISIONS)?;
    let (ids, batch) = decision_targets(args, decision)?;
    // Compared *before* the write. Afterwards the label is itself a
    // recorded human act, and every verb that could correct one refuses
    // by design — so this is the only moment a slip is still fixable.
    let resembling = match opt_str(args, "human") {
        Some(human) => engine
            .deciders_resembling(&human)
            .map_err(|error| error.to_string())?,
        None => Vec::new(),
    };
    if batch {
        let human = required_for(
            args,
            "human",
            decision,
            "the person applying this batch act.",
        )?;
        let reason = required_for(
            args,
            "reason",
            decision,
            "the rationale shared by every design in this batch.",
        )?;
        let action = DesignActionKind::from_tag(decision)
            .ok_or_else(|| format!("decision={decision} does not support batch targets"))?;
        let items = engine
            .apply_design_actions(&ids, action, &human, &reason)
            .map_err(|error| error.to_string())?;
        let mut values = items
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        if !resembling.is_empty() {
            for value in &mut values {
                attach_attribution_warning(value, args, &resembling);
            }
        }
        return ok(&values);
    }
    let mut items = Vec::with_capacity(ids.len());
    for id in &ids {
        let item = match decision {
            "accept" => engine.accept_design(
                id,
                &required_for(
                    args,
                    "human",
                    decision,
                    "the human reviewer's identity, which must differ from the proposing agent.",
                )?,
            ),
            "reject" => engine.reject_design(
                id,
                &required_for(args, "human", decision, "the person refusing the design.")?,
                &required_for(args, "reason", decision, "why the design was refused.")?,
            ),
            "defer" => engine.defer_design(
                id,
                &required_for(args, "human", decision, "the person parking the design.")?,
                &required_for(args, "reason", decision, "why the design is not for now.")?,
            ),
            "resume" => engine.resume_design(
                id,
                &required_for(args, "human", decision, "the person returning the design.")?,
                &required_for(
                    args,
                    "reason",
                    decision,
                    "why the design is returning to the working board.",
                )?,
            ),
            "retire" => engine.retire_design(
                id,
                &required_for(args, "human", decision, "the person retiring the record.")?,
                &required_for(
                    args,
                    "reason",
                    decision,
                    "why this record is no longer a live entry.",
                )?,
            ),
            "supersede" => engine.supersede_design(
                id,
                &required_for(
                    args,
                    "superseded_by",
                    decision,
                    "the registered design that replaces this one.",
                )?,
                &required_for(
                    args,
                    "human",
                    decision,
                    "the person recording the supersession.",
                )?,
            ),
            "reopen" => engine.reopen_undecided_design(id),
            "attribute" => engine.attribute_design_decision(
                id,
                &required_for(args, "human", decision, "the person who made the decision.")?,
            ),
            other => unreachable!("one_of refused every value but {DECISIONS:?}, not {other}"),
        };
        let item = item.map_err(|error| error.to_string())?;
        let mut value = serde_json::to_value(&item).map_err(|error| error.to_string())?;
        if !resembling.is_empty() {
            attach_attribution_warning(&mut value, args, &resembling);
        }
        items.push(value);
    }
    ok(&items.remove(0))
}

fn decision_targets(args: &Value, decision: &str) -> Result<(Vec<String>, bool), String> {
    let id = opt_str(args, "id");
    let ids = args.get("ids");
    if id.is_some() && ids.is_some() {
        return Err("design_decide takes exactly one of id or ids, not both.".to_string());
    }
    if let Some(id) = id {
        return Ok((vec![id], false));
    }
    let ids = ids.ok_or_else(|| "design_decide requires exactly one of id or ids.".to_string())?;
    if !BATCH_DECISIONS.contains(&decision) {
        return Err(format!(
            "decision={decision} requires one id; ids is supported only for {}.",
            BATCH_DECISIONS.join(", ")
        ));
    }
    let values = ids
        .as_array()
        .ok_or_else(|| "ids must be a non-empty array of design ids.".to_string())?;
    let mut parsed = Vec::with_capacity(values.len());
    for value in values {
        let id = value
            .as_str()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| "ids must contain only non-empty design ids.".to_string())?;
        if parsed.iter().any(|existing| existing == id) {
            return Err(format!("ids contains duplicate design id: {id}"));
        }
        parsed.push(id.to_string());
    }
    if parsed.is_empty() {
        return Err("ids must be a non-empty array of design ids.".to_string());
    }
    Ok((parsed, true))
}

fn attach_attribution_warning(value: &mut Value, args: &Value, resembling: &[String]) {
    // Advisory, never a refusal: an unverifiable identity can only be
    // compared, and rejecting a genuinely new reviewer whose name resembles
    // an existing one is worse than the typo it would catch.
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "attribution_warning".to_string(),
            json!({
                "recorded": opt_str(args, "human"),
                "resembles": resembling,
                "advice": "this decider label is one edit from one already in the ledger, \
                           which is usually a typo for it. Nothing rewrites a recorded \
                           human act afterwards, so correct it now or accept it as a \
                           distinct person.",
            }),
        );
    }
}

//! Knowledge tool definitions and dispatch.

use super::{i64_arg, ok, opt_str, req_str, str_array, text};
use lodestar_core::{Lodestar, SignalPromotion};
use serde_json::{json, Value};

pub(super) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "record_knowledge",
            "description": "Record a consolidated learned regularity (durable but revalidated). Prefer 'consolidate' for gated promotion.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "statement": { "type": "string" },
                    "evidence": { "type": "string", "description": "JSON provenance." },
                    "half_life_hours": { "type": "number" }
                },
                "required": ["statement"]
            }
        }),
        json!({
            "name": "consolidate",
            "description": "Gated promotion of a discovered regularity into durable knowledge. Stores nothing unless the evidence clears count + span thresholds (signal, not coincidence).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "statement": { "type": "string" },
                    "evidence_node_ids": { "type": "array", "items": { "type": "string" } },
                    "first_seen": { "type": "integer" },
                    "last_seen": { "type": "integer" }
                },
                "required": ["statement", "evidence_node_ids", "first_seen", "last_seen"]
            }
        }),
        json!({
            "name": "promote_signals",
            "description": "Promotion bridge (ADR-0022): feed MindLeak proven-signal candidates (opaque node ids + provenance span) into the gated consolidator in one call. Reuses the count + span gate; builds a deterministic templated statement when a candidate has none. Returns the knowledge that cleared the gate.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "candidates": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "subject": { "type": "string", "description": "Short label for the templated statement." },
                                "evidence_node_ids": { "type": "array", "items": { "type": "string" } },
                                "first_seen": { "type": "integer" },
                                "last_seen": { "type": "integer" },
                                "statement": { "type": "string", "description": "Optional pre-distilled summary; omit for a deterministic template." }
                            },
                            "required": ["subject", "evidence_node_ids", "first_seen", "last_seen"]
                        }
                    }
                },
                "required": ["candidates"]
            }
        }),
        json!({
            "name": "reconfirm_knowledge",
            "description": "Re-confirm a knowledge node with fresh evidence (resets its revalidation clock).",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }
        }),
        json!({
            "name": "prune_knowledge",
            "description": "Purge knowledge that decayed below the threshold without reconfirmation.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "active_knowledge",
            "description": "Read what this repository has learned and not yet forgotten. Knowledge was write-only from this surface: it could be recorded, promoted, reconfirmed and pruned, but never read, so the only consumer was the conformance advisory and an agent could not find out what was already known before rediscovering it. Each entry reports the nodes it references, because that is the sole thing the advisory matches on - a record whose evidence carries no `nodes` array is stored, counted, and can never surface, and `surfaces: false` is how you see that rather than guessing.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "node": { "type": "string", "description": "Return only knowledge referencing this node id, e.g. artifact:crates/lodestar-core/src/llm.rs - what is known about the thing you are about to change." },
                    "contains": { "type": "string", "description": "Return only knowledge whose statement contains this text, matched case-insensitively." }
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
        "record_knowledge" => Some((|| {
            let k = engine
                .record_knowledge(
                    req_str(args, "statement")?,
                    opt_str(args, "evidence")
                        .unwrap_or_else(|| "{}".to_string())
                        .as_str(),
                    args.get("half_life_hours").and_then(Value::as_f64),
                )
                .map_err(|e| e.to_string())?;
            // Whether this record can ever be read, answered here rather than
            // left for someone to discover later. The advisory matches on
            // referenced nodes and nothing else, so a record naming none is
            // stored, counted, and permanently unreachable — and the failure is
            // silent in the one direction that matters, because the agent it
            // was written for never learns it exists.
            //
            // Said at write time because that is the only moment anyone still
            // has the nodes to hand. `active_knowledge` already reports
            // `surfaces`, but reading it requires suspecting the problem first,
            // and an agent recording a lesson for a colleague has no reason to.
            // Measured before this landed: 3 of 17 active records were
            // invisible, one of them the cost of skipping the mandatory advise
            // pre-flight — written precisely so the next agent would not repeat
            // it, and unable to reach them.
            let nodes = k.referenced_nodes();
            let mut body = serde_json::to_value(&k).map_err(|e| e.to_string())?;
            if let Some(object) = body.as_object_mut() {
                object.insert("surfaces".to_string(), json!(!nodes.is_empty()));
                if nodes.is_empty() {
                    object.insert(
                        "surfaces_advice".to_string(),
                        json!(
                            "This record can never reach another agent: the conformance advisory \
                             matches on referenced nodes, and this one names none. Record it again \
                             with evidence carrying a `nodes` array of the artifact:/symbol: ids it \
                             is about — the ids you were just working on. It is kept either way, \
                             because deleting an agent's stated lesson is worse than storing an \
                             unreachable one."
                        ),
                    );
                }
            }
            ok(&body)
        })()),
        "consolidate" => Some((|| {
            let promoted = engine
                .consolidate(
                    req_str(args, "statement")?,
                    &str_array(args, "evidence_node_ids"),
                    i64_arg(args, "first_seen", 0),
                    i64_arg(args, "last_seen", 0),
                )
                .map_err(|e| e.to_string())?;
            match promoted {
                Some(k) => ok(&k),
                None => text("not promoted: evidence below count/span threshold".to_string()),
            }
        })()),
        "promote_signals" => Some((|| {
            let candidates = args
                .get("candidates")
                .and_then(Value::as_array)
                .ok_or_else(|| "missing required array arg: candidates".to_string())?
                .iter()
                .map(|candidate| SignalPromotion {
                    subject: candidate
                        .get("subject")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    evidence_node_ids: str_array(candidate, "evidence_node_ids"),
                    first_seen: i64_arg(candidate, "first_seen", 0),
                    last_seen: i64_arg(candidate, "last_seen", 0),
                    statement: opt_str(candidate, "statement"),
                })
                .collect::<Vec<_>>();
            let promoted = engine
                .promote_signals(&candidates)
                .map_err(|e| e.to_string())?;
            ok(&promoted)
        })()),
        "active_knowledge" => Some((|| {
            let node = opt_str(args, "node");
            let contains = opt_str(args, "contains").map(|c| c.to_lowercase());
            let known = engine.active_knowledge().map_err(|e| e.to_string())?;

            let rows: Vec<Value> = known
                .iter()
                .filter(|k| match node.as_deref() {
                    Some(wanted) => k.referenced_nodes().iter().any(|n| n == wanted),
                    None => true,
                })
                .filter(|k| match contains.as_deref() {
                    Some(needle) => k.statement.to_lowercase().contains(needle),
                    None => true,
                })
                .map(|k| {
                    let nodes = k.referenced_nodes();
                    json!({
                        "id": k.id,
                        "statement": k.statement,
                        "weight": k.weight,
                        "half_life_hours": k.half_life_hours,
                        "confirmed_at": k.confirmed_at,
                        "nodes": nodes,
                        // The advisory matches on referenced nodes and nothing
                        // else, so knowledge naming none can never reach the
                        // agent it was written for. Said plainly rather than
                        // left to be inferred from an empty array.
                        "surfaces": !nodes.is_empty(),
                    })
                })
                .collect();

            let silent = rows
                .iter()
                .filter(|r| r["surfaces"] == json!(false))
                .count();
            ok(&json!({
                "count": rows.len(),
                "never_surfaces": silent,
                "knowledge": rows,
            }))
        })()),
        "reconfirm_knowledge" => Some((|| {
            let reconfirmed = engine
                .reconfirm_knowledge(req_str(args, "id")?)
                .map_err(|e| e.to_string())?;
            ok(&json!({ "reconfirmed": reconfirmed }))
        })()),
        "prune_knowledge" => Some((|| {
            let pruned = engine.prune_knowledge().map_err(|e| e.to_string())?;
            ok(&json!({ "pruned": pruned }))
        })()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::super::call;
    use lodestar_core::Lodestar;
    use serde_json::{json, Value};

    fn payload(result: Value) -> Value {
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    fn record(engine: &Lodestar, statement: &str, evidence: &str) -> Value {
        payload(
            call(
                engine,
                &json!({
                    "name": "record_knowledge",
                    "arguments": { "statement": statement, "evidence": evidence },
                }),
            )
            .unwrap(),
        )
    }

    /// Recording knowledge that names no nodes says so, at the moment of
    /// writing.
    ///
    /// The conformance advisory matches on referenced nodes and nothing else,
    /// so a record naming none is stored, counted, and can never reach the
    /// agent it was written for. Nothing reported that: `active_knowledge`
    /// exposes `surfaces`, but reading it requires already suspecting the
    /// problem, and an agent recording a lesson for a colleague has no reason
    /// to. Measured before this landed — 3 of 17 active records were invisible,
    /// one of them the cost of skipping the mandatory `advise` pre-flight,
    /// written precisely so the next agent would not repeat it.
    #[test]
    fn knowledge_that_can_never_be_read_says_so_when_it_is_written() {
        let engine = Lodestar::open_in_memory().unwrap();

        let silent = record(&engine, "a lesson addressed to nobody", "{}");

        assert_eq!(
            silent["surfaces"],
            json!(false),
            "a record naming no nodes cannot surface and must say so: {silent}"
        );
        let advice = silent["surfaces_advice"]
            .as_str()
            .expect("an unreachable record explains what to do about it");
        assert!(
            advice.contains("nodes"),
            "the advice must name the missing field: {advice}"
        );
    }

    /// The record is kept either way. Refusing it would lose an agent's stated
    /// lesson to a formatting mistake, which is worse than storing one that
    /// cannot be matched.
    #[test]
    fn an_unreachable_record_is_still_recorded() {
        let engine = Lodestar::open_in_memory().unwrap();

        let silent = record(&engine, "kept despite naming nobody", "{}");
        assert!(silent["id"].is_string(), "it still has an id: {silent}");

        let active = payload(
            call(
                &engine,
                &json!({ "name": "active_knowledge", "arguments": {} }),
            )
            .unwrap(),
        );
        let statements = active.to_string();
        assert!(
            statements.contains("kept despite naming nobody"),
            "the record survives: {statements}"
        );
    }

    /// A record that does name nodes reports that it will surface, so the
    /// field means something rather than always warning.
    #[test]
    fn a_record_naming_nodes_reports_that_it_surfaces() {
        let engine = Lodestar::open_in_memory().unwrap();

        let reachable = record(
            &engine,
            "a lesson about a real file",
            "{\"nodes\": [\"artifact:crates/lodestar-mcp/src/tools/knowledge.rs\"]}",
        );

        assert_eq!(
            reachable["surfaces"],
            json!(true),
            "naming a node makes it reachable: {reachable}"
        );
        assert!(
            reachable["surfaces_advice"].is_null(),
            "a reachable record is not nagged: {reachable}"
        );
    }
}

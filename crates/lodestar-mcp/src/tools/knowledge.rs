//! Knowledge tool definitions and dispatch.

use super::{i64_arg, ok, opt_str, req_str, str_array, text};
use lodestar_core::{KnowledgeReach, Lodestar, SignalPromotion};
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
                    // Said here rather than only in the reply, because this is
                    // where the caller decides what to send. The advisory
                    // matches on referenced nodes and nothing else, so evidence
                    // without a `nodes` array produces a record that is stored,
                    // counted, decayed, and unreachable — and the call looks
                    // exactly like one that worked. Describing this field as
                    // "JSON provenance" was true and told nobody the one thing
                    // that decides whether the lesson ever arrives: measured
                    // when this was written, 67 of 170 active records named no
                    // nodes, among them the ones about skipping the ADR-0029
                    // pre-flight and about facade tests missing MCP wiring.
                    "evidence": {
                        "type": "string",
                        "description": "JSON provenance. Carry a `nodes` array of the artifact:/symbol: ids this is about, e.g. {\"nodes\":[\"artifact:src/a.rs\"],\"method\":\"how you know\"} — a lesson naming nodes reaches anyone whose evidence touches them, unconditionally. Failing that, a `goal` (or a task id the ledger still knows) reaches work under that goal, but competes with every other node-less lesson there for a capped number of slots. With neither, the record is stored and arrives nowhere. The reply's `reach` and `surfaces` fields tell you which you got."
                    },
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
            "description": "Read what this repository has learned and not yet forgotten. Knowledge was write-only from this surface: it could be recorded, promoted, reconfirmed and pruned, but never read, so the only consumer was the conformance advisory and an agent could not find out what was already known before rediscovering it. Each entry reports how it reaches an agent: `reach` is `node` when the nodes it names carry it unconditionally, `goal:<id>` when it names none but still reaches work under the goal it was learned under (a capped, contended path), and `none` when it has neither and arrives nowhere.",
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
            // left for someone to discover later. The failure is silent in the
            // one direction that matters, because the agent it was written for
            // never learns it exists.
            //
            // Said at write time because that is the only moment anyone still
            // has the nodes to hand. `active_knowledge` also reports `reach`,
            // but reading it requires suspecting the problem first, and an
            // agent recording a lesson for a colleague has no reason to.
            //
            // The judgement itself belongs to the facade, not here: this reply,
            // `active_knowledge` and `scripts/silent-knowledge.mjs` each used to
            // decide it independently, and all three were falsified together the
            // day a second reaching path landed.
            let reach = engine.knowledge_reach(&k).map_err(|e| e.to_string())?;
            let mut body = serde_json::to_value(&k).map_err(|e| e.to_string())?;
            if let Some(object) = body.as_object_mut() {
                object.insert("surfaces".to_string(), json!(reach.reaches()));
                match &reach {
                    KnowledgeReach::ByNode => {}
                    KnowledgeReach::ByGoal(goal) => {
                        object.insert(
                            "surfaces_advice".to_string(),
                            json!(format!(
                                "This record names no node, so it reaches agents only through the \
                                 goal it was learned under ({goal}) — and only while it is among \
                                 the strongest few lessons there, because that path is capped per \
                                 check. Recording it again with an evidence `nodes` array of the \
                                 artifact:/symbol: ids it is about would make it arrive \
                                 unconditionally, for anyone touching those files."
                            )),
                        );
                    }
                    KnowledgeReach::Unreachable => {
                        object.insert(
                            "surfaces_advice".to_string(),
                            json!(
                                "This record can reach nobody: the conformance advisory carries a \
                                 lesson by the nodes it names or by the goal it was learned under, \
                                 and this one has neither. Record it again with evidence carrying a \
                                 `nodes` array of the artifact:/symbol: ids it is about — the ids \
                                 you were just working on — or a `goal`. It is kept either way, \
                                 because deleting an agent's stated lesson is worse than storing an \
                                 unreachable one."
                            ),
                        );
                    }
                }
                object.insert(
                    "reach".to_string(),
                    json!(match &reach {
                        KnowledgeReach::ByNode => "node".to_string(),
                        KnowledgeReach::ByGoal(goal) => format!("goal:{goal}"),
                        KnowledgeReach::Unreachable => "none".to_string(),
                    }),
                );
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
                    // Which of the two advisory paths carries this lesson, said
                    // plainly rather than left to be inferred from an empty
                    // array. Inferring it from `nodes` alone is what made this
                    // surface, `record_knowledge` and the audit script all
                    // report the same wrong answer.
                    let reach = engine.knowledge_reach(k).map_err(|e| e.to_string())?;
                    Ok(json!({
                        "id": k.id,
                        "statement": k.statement,
                        "weight": k.weight,
                        "half_life_hours": k.half_life_hours,
                        "confirmed_at": k.confirmed_at,
                        "nodes": nodes,
                        "reach": match &reach {
                            KnowledgeReach::ByNode => "node".to_string(),
                            KnowledgeReach::ByGoal(goal) => format!("goal:{goal}"),
                            KnowledgeReach::Unreachable => "none".to_string(),
                        },
                        "surfaces": reach.reaches(),
                    }))
                })
                .collect::<std::result::Result<Vec<Value>, String>>()?;

            let unreachable = rows
                .iter()
                .filter(|r| r["surfaces"] == json!(false))
                .count();
            let by_goal_only = rows
                .iter()
                .filter(|r| {
                    r["reach"]
                        .as_str()
                        .is_some_and(|reach| reach.starts_with("goal:"))
                })
                .count();
            ok(&json!({
                "count": rows.len(),
                "never_surfaces": unreachable,
                "reaches_by_goal_only": by_goal_only,
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

    /// The advertised schema tells a caller what evidence must carry.
    ///
    /// The reply's `reach` warning is a backstop, and it arrives after the
    /// record exists. The schema is where the caller decides what to send, so
    /// it has to name the `nodes` array and say what omitting it costs — not
    /// that the record dies, which was never true of every such record, but
    /// that it falls back to a capped, contended path.
    #[test]
    fn the_schema_says_evidence_must_name_nodes() {
        let record = super::definitions()
            .into_iter()
            .find(|tool| tool["name"] == json!("record_knowledge"))
            .expect("record_knowledge is advertised");
        let described = record["inputSchema"]["properties"]["evidence"]["description"]
            .as_str()
            .expect("evidence documents itself");

        assert!(
            described.contains("nodes"),
            "the caller must be told which field carries the ids: {described}"
        );
        assert!(
            described.contains("artifact:"),
            "and what an id looks like: {described}"
        );
        assert!(
            described.contains("goal"),
            "and that a goal is the other way a lesson reaches anyone: {described}"
        );
    }

    /// Recording knowledge that can reach nobody says so, at the moment of
    /// writing.
    ///
    /// A lesson reaches an agent by the nodes it names or by the goal it was
    /// learned under. One naming neither is stored, counted, and arrives
    /// nowhere. Nothing reported that until this landed, and reading
    /// `active_knowledge` to find out requires already suspecting the problem.
    #[test]
    fn knowledge_that_can_reach_nobody_says_so_when_it_is_written() {
        let engine = Lodestar::open_in_memory().unwrap();

        let unreachable = record(&engine, "a lesson addressed to nobody", "{}");

        assert_eq!(
            unreachable["surfaces"],
            json!(false),
            "a record naming neither a node nor a goal must say so: {unreachable}"
        );
        assert_eq!(unreachable["reach"], json!("none"));
        let advice = unreachable["surfaces_advice"]
            .as_str()
            .expect("an unreachable record explains what to do about it");
        assert!(
            advice.contains("nodes"),
            "the advice must name the missing field: {advice}"
        );
    }

    /// A lesson naming no node but declaring a goal is NOT unreachable, and
    /// must not be reported as though it were.
    ///
    /// This is the regression that motivated the change. Reachability was
    /// computed as `!nodes.is_empty()` in three separate places, so when the
    /// goal path landed all three began reporting records as dead that were
    /// arriving on every check under that goal — 68 of 210 here, against 12
    /// genuinely unreachable. The advice must still push toward nodes, because
    /// the goal path is capped and contended.
    #[test]
    fn a_lesson_naming_a_goal_reaches_that_goal_rather_than_nobody() {
        let engine = Lodestar::open_in_memory().unwrap();

        let by_goal = record(
            &engine,
            "a lesson learned serving an objective",
            r#"{"goal":"goal:durable-intent-plane@constitution:v3"}"#,
        );

        assert_eq!(
            by_goal["surfaces"],
            json!(true),
            "it reaches work under its goal: {by_goal}"
        );
        assert_eq!(
            by_goal["reach"],
            json!("goal:goal:durable-intent-plane@constitution:v3")
        );
        let advice = by_goal["surfaces_advice"]
            .as_str()
            .expect("the contended path still deserves advice");
        assert!(
            advice.contains("capped") || advice.contains("strongest"),
            "and that advice must say the path is contended: {advice}"
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

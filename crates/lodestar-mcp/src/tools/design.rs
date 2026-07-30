//! The design surface (ADR-0023): four verbs over the design-item ledger.
//!
//! This was fifteen tools. Fifteen names is not fifteen capabilities — it is one
//! capability with its argument space spelled out in the tool list, which is the
//! most expensive place to spell anything. Every one of those names cost an
//! agent a slot in a finite tool budget, and the distinctions between them
//! (`retire` versus `reject` versus `supersede`) were never discoverable from
//! the names anyway; they live in the descriptions, and they still do.
//!
//! So the cluster is `design_register`, `design_decide`, `design_promote` and
//! `design_query`, and every refusal a separate name used to encode is now an
//! argument validation carrying the same message. Nothing became more
//! permissive: the guards are engine-side and untouched, so attribution still
//! refuses to overwrite a recorded name and reopening still defers to
//! materialisation (ADR-0051). The old names answer for one minor version and
//! say which call to make instead — see `RENAMED`.

use lodestar_core::design::DesignMetadata;
use lodestar_core::{DesignStatus, Lodestar};
use serde_json::{json, Value};

use super::design_materialization::{materialization_plan_schema, parse_materialization_plan};
use super::{bool_arg, ok, one_of, opt_str, renamed, req_str, required_for, Renamed};

const DECISIONS: [&str; 6] = [
    "accept",
    "reject",
    "retire",
    "supersede",
    "reopen",
    "attribute",
];
const STEPS: [&str; 3] = ["plan", "materialize", "revise"];
const VIEWS: [&str; 4] = ["board", "ledger", "promotion", "history"];

pub(super) fn definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "design_register",
            "description": "Put ADRs into the design ledger. One design (adr_path + title) registers it as proposed and tainted under ADR-0023: invisible to next_task and the executive board, present on the design board, id derived from the ADR path, attributed to the registering session. A batch (designs) instead reconciles structured repository ADR metadata idempotently — it never invokes a model, never creates goals or tasks, and existing human decisions and promotion state always win. Supply one shape or the other, not both.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "adr_path": { "type": "string", "description": "Path to the ADR, e.g. docs/adr/0023-....md. Registers a single design." },
                    "title": { "type": "string", "description": "Required with adr_path." },
                    "summary": { "type": "string", "description": "What the design decides; used later to decompose it into tasks." },
                    "designs": {
                        "type": "array",
                        "description": "Reconcile many at once from repository metadata.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "adr_path": { "type": "string" },
                                "title": { "type": "string" },
                                "summary": { "type": "string" },
                                "status": { "type": "string", "enum": ["proposed", "accepted", "rejected"] },
                                "proposed_by": { "type": "string" }
                            },
                            "required": ["adr_path", "title", "status"]
                        }
                    }
                }
            }
        }),
        json!({
            "name": "design_decide",
            "description": "Every attributed human act on a design's status. decision=accept is the guarded acceptance (ADR-0023): the design becomes accepted with promotion state 'pending', it runs no code conformance and creates no tasks, and no agent may accept its own design — materialise the work with design_promote. decision=reject is durable and auditable, spawns no work, and requires a rationale. decision=retire (ADR-0042) says the record should not have existed — use it for an orphan row whose ADR was renamed or renumbered; retiring is not deleting, the row keeps its id, path, decision, decider and materialization history and simply leaves the working board. Nothing retires a design automatically: a missing ADR file is never evidence, because worktrees on different branches share one database and a file absent from one checkout is alive on another. decision=supersede (ADR-0050) says the decision was made, it held, and something better replaced it — the row stays accepted and gains a link to its successor, exactly as a goal's superseded_by works, so a live design is one with no superseded_by; only an accepted design with a recorded decider can be superseded, and the replacement must already be registered. decision=reopen (ADR-0047) returns a design whose status nobody ever decided to 'proposed' so a human can decide it, and is refused for a row that already carries a decider, once promotion has materialised work, or after retirement — it is not an undo. decision=attribute (ADR-0051) records who made a decision the ledger already asserts but attributes to nobody; it is not a decision, the status, reason and promotion state are untouched, and it takes exactly the rows reopen refuses.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Design item id, e.g. design:0023-design-board-accept-bridge" },
                    "decision": { "type": "string", "enum": ["accept", "reject", "retire", "supersede", "reopen", "attribute"] },
                    "human": { "type": "string", "description": "The person making the act. Required for every decision except reopen, which records none." },
                    "reason": { "type": "string", "description": "Required for reject and retire." },
                    "superseded_by": { "type": "string", "description": "Required for supersede: the registered design that replaces this one." }
                },
                "required": ["id", "decision"]
            }
        }),
        json!({
            "name": "design_promote",
            "description": "Materialise an accepted design into work, in reviewed steps. step=plan produces a read-only suggested create plan under one objective and never creates tasks — the caller must show and review it. step=materialize applies exactly one explicit human-reviewed create/link/no_work plan; idempotent retries return the same revision and never duplicate work. step=revise appends an attributed repair revision and replaces the current provenance projection, leaving prior plans and tasks durable; a non-empty rationale is required.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Accepted design item id." },
                    "step": { "type": "string", "enum": ["plan", "materialize", "revise"] },
                    "objective_goal_id": { "type": "string", "description": "Required for plan." },
                    "plan": materialization_plan_schema(),
                    "human": { "type": "string", "description": "Required for revise: the person recording the repair." }
                },
                "required": ["id", "step"]
            }
        }),
        json!({
            "name": "design_query",
            "description": "Read the design ledger. view=board lists actionable items: proposed ADRs awaiting a human decision plus accepted designs awaiting or retrying promotion — distinct from the executive task board. view=ledger reads the durable record, optionally filtered by status, including historical and materialized items; retired records (ADR-0042) are omitted unless include_retired. view=promotion reads the objectives, tasks and constraints materialized for one design, returning null while proposed or pending and never invoking planning. view=history reads every immutable materialization and repair revision for one design, oldest first. All read-only.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "view": { "type": "string", "enum": ["board", "ledger", "promotion", "history"] },
                    "id": { "type": "string", "description": "Required for promotion and history." },
                    "status": { "type": "string", "enum": ["proposed", "accepted", "rejected"], "description": "Filters ledger." },
                    "include_retired": { "type": "boolean", "description": "Include retired records in the ledger audit view. Defaults to false." }
                },
                "required": ["view"]
            }
        }),
    ]
}

pub(super) fn dispatch(
    engine: &Lodestar,
    name: &str,
    args: &Value,
) -> Option<Result<Value, String>> {
    if let Some(renamed) = renamed(&RENAMED, name) {
        let translated = renamed.translate(args);
        let answer = dispatch(engine, renamed.new, &translated)?;
        return Some(answer.map(|result| renamed.teach(result)));
    }

    match name {
        "design_register" => Some((|| {
            if args.get("designs").is_some() {
                if args.get("adr_path").is_some() {
                    return Err("design_register takes either one design (adr_path, title) \
                                or a reconcile batch (designs), not both."
                        .to_string());
                }
                let designs = parse_design_metadata(args)?;
                return ok(&engine
                    .reconcile_designs(&designs)
                    .map_err(|error| error.to_string())?);
            }
            ok(&engine
                .register_design(
                    req_str(args, "adr_path")?,
                    req_str(args, "title")?,
                    opt_str(args, "summary").unwrap_or_default().as_str(),
                    Some(req_str(args, "agent")?),
                )
                .map_err(|error| error.to_string())?)
        })()),
        "design_decide" => Some((|| {
            let id = req_str(args, "id")?;
            let decision = one_of(args, "decision", &DECISIONS)?;
            // Compared *before* the write. Afterwards the label is itself a
            // recorded human act, and every verb that could correct one refuses
            // by design — so this is the only moment a slip is still fixable.
            let resembling = match opt_str(args, "human") {
                Some(human) => engine
                    .deciders_resembling(&human)
                    .map_err(|error| error.to_string())?,
                None => Vec::new(),
            };
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
            if resembling.is_empty() {
                return ok(&item);
            }
            // Advisory, never a refusal: an unverifiable identity can only be
            // compared, and rejecting a genuinely new reviewer whose name
            // resembles an existing one is worse than the typo it would catch.
            let mut value = serde_json::to_value(&item).map_err(|error| error.to_string())?;
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
            ok(&value)
        })()),
        "design_promote" => Some((|| {
            let id = req_str(args, "id")?;
            let step = one_of(args, "step", &STEPS)?;
            match step {
                "plan" => ok(&engine
                    .plan_design_promotion(
                        id,
                        &required_for(
                            args,
                            "objective_goal_id",
                            step,
                            "the objective the work hangs under.",
                        )?,
                    )
                    .map_err(|error| error.to_string())?),
                "materialize" => ok(&engine
                    .promote_design(id, &parse_materialization_plan(args)?)
                    .map_err(|error| error.to_string())?),
                "revise" => ok(&engine
                    .revise_design_promotion(
                        id,
                        &required_for(args, "human", step, "the person recording the repair.")?,
                        &parse_materialization_plan(args)?,
                    )
                    .map_err(|error| error.to_string())?),
                other => unreachable!("one_of refused every value but {STEPS:?}, not {other}"),
            }
        })()),
        "design_query" => Some((|| {
            let view = one_of(args, "view", &VIEWS)?;
            match view {
                "board" => ok(&engine.design_board().map_err(|error| error.to_string())?),
                "ledger" => {
                    let status = match opt_str(args, "status") {
                        Some(value) => Some(
                            DesignStatus::from_tag(&value)
                                .ok_or_else(|| format!("unknown design status: {value}"))?,
                        ),
                        None => None,
                    };
                    ok(&engine
                        .list_design_items(status, bool_arg(args, "include_retired", false))
                        .map_err(|error| error.to_string())?)
                }
                "promotion" => ok(&engine
                    .design_promotion(&required_for(args, "id", view, "the design to read.")?)
                    .map_err(|error| error.to_string())?),
                "history" => ok(&engine
                    .design_materialization_history(&required_for(
                        args,
                        "id",
                        view,
                        "the design to read.",
                    )?)
                    .map_err(|error| error.to_string())?),
                other => unreachable!("one_of refused every value but {VIEWS:?}, not {other}"),
            }
        })()),
        _ => None,
    }
}

/// The fifteen names this cluster used to answer to, and the call to make now.
///
/// `reconcile_designs` still answers without a session, where `design_register`
/// now requires one, because a deprecation that changes behaviour teaches the
/// wrong lesson.
pub(super) const RENAMED: [Renamed; 15] = [
    Renamed {
        old: "register_design",
        new: "design_register",
        key: "",
        value: "",
    },
    Renamed {
        old: "reconcile_designs",
        new: "design_register",
        key: "",
        value: "",
    },
    Renamed {
        old: "accept_design",
        new: "design_decide",
        key: "decision",
        value: "accept",
    },
    Renamed {
        old: "reject_design",
        new: "design_decide",
        key: "decision",
        value: "reject",
    },
    Renamed {
        old: "retire_design",
        new: "design_decide",
        key: "decision",
        value: "retire",
    },
    Renamed {
        old: "supersede_design",
        new: "design_decide",
        key: "decision",
        value: "supersede",
    },
    Renamed {
        old: "reopen_undecided_design",
        new: "design_decide",
        key: "decision",
        value: "reopen",
    },
    Renamed {
        old: "attribute_design_decision",
        new: "design_decide",
        key: "decision",
        value: "attribute",
    },
    Renamed {
        old: "plan_design_promotion",
        new: "design_promote",
        key: "step",
        value: "plan",
    },
    Renamed {
        old: "promote_design",
        new: "design_promote",
        key: "step",
        value: "materialize",
    },
    Renamed {
        old: "revise_design_promotion",
        new: "design_promote",
        key: "step",
        value: "revise",
    },
    Renamed {
        old: "design_board",
        new: "design_query",
        key: "view",
        value: "board",
    },
    Renamed {
        old: "list_designs",
        new: "design_query",
        key: "view",
        value: "ledger",
    },
    Renamed {
        old: "design_promotion",
        new: "design_query",
        key: "view",
        value: "promotion",
    },
    Renamed {
        old: "design_materialization_history",
        new: "design_query",
        key: "view",
        value: "history",
    },
];

fn parse_design_metadata(args: &Value) -> Result<Vec<DesignMetadata>, String> {
    let designs = args
        .get("designs")
        .cloned()
        .ok_or_else(|| "missing required array arg: designs".to_string())?;
    serde_json::from_value(designs).map_err(|error| format!("invalid design metadata: {error}"))
}

#[cfg(test)]
mod tests {
    use super::super::{call, list};
    use super::RENAMED;
    use lodestar_core::llm::LlmClient;
    use lodestar_core::{GoalKind, Lodestar};
    use serde_json::{json, Value};

    fn engine() -> Lodestar {
        // Unreachable model so the plan step takes its deterministic
        // single-task fallback — independent of any ambient local model.
        Lodestar::open_in_memory()
            .unwrap()
            .with_llm(LlmClient::unreachable())
    }

    fn payload(result: Value) -> Value {
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    fn register(engine: &Lodestar, adr_path: &str, title: &str) -> String {
        payload(
            call(
                engine,
                &json!({
                    "name": "design_register",
                    "arguments": { "adr_path": adr_path, "title": title, "agent": "planner" }
                }),
            )
            .unwrap(),
        )["id"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn decide(engine: &Lodestar, id: &str, decision: &str, human: &str) -> Result<Value, String> {
        call(
            engine,
            &json!({
                "name": "design_decide",
                "arguments": { "id": id, "decision": decision, "human": human }
            }),
        )
    }

    fn board(engine: &Lodestar) -> Value {
        payload(
            call(
                engine,
                &json!({ "name": "design_query", "arguments": { "view": "board" } }),
            )
            .unwrap(),
        )
    }

    #[test]
    fn register_accept_promote_round_trips_through_the_four_verbs() {
        let engine = engine();
        let registered = payload(
            call(
                &engine,
                &json!({
                    "name": "design_register",
                    "arguments": {
                        "adr_path": "docs/adr/0023-design-board-accept-bridge.md",
                        "title": "Accept bridge",
                        "summary": "human accept completes design work",
                        "agent": "planner"
                    }
                }),
            )
            .unwrap(),
        );
        assert_eq!(registered["status"], "proposed");
        let id = registered["id"].as_str().unwrap().to_string();

        assert_eq!(board(&engine).as_array().unwrap().len(), 1);

        let ledger = payload(
            call(
                &engine,
                &json!({
                    "name": "design_query",
                    "arguments": { "view": "ledger", "status": "proposed" }
                }),
            )
            .unwrap(),
        );
        assert_eq!(ledger.as_array().unwrap().len(), 1);

        // Accept is decision-only: accepted + pending, no tasks, still visible
        // for promotion or retry.
        let accepted = payload(decide(&engine, &id, "accept", "reviewer").unwrap());
        assert_eq!(accepted["status"], "accepted");
        assert_eq!(accepted["promotion_status"], "pending");
        let pending = board(&engine);
        assert_eq!(pending.as_array().unwrap().len(), 1);
        assert_eq!(pending[0]["promotion_status"], "pending");

        // Planning is read-only; only the reviewed plan materialises work.
        let objective = engine
            .define_goal(GoalKind::Objective, "Ship the bridge", "wire it", None)
            .unwrap();
        let plan = payload(
            call(
                &engine,
                &json!({
                    "name": "design_promote",
                    "arguments": { "id": id, "step": "plan", "objective_goal_id": objective.id }
                }),
            )
            .unwrap(),
        );
        assert!(engine.next_task().unwrap().is_none());
        let promo = payload(
            call(
                &engine,
                &json!({
                    "name": "design_promote",
                    "arguments": { "id": id, "step": "materialize", "plan": plan }
                }),
            )
            .unwrap(),
        );
        assert_eq!(promo["item"]["promotion_status"], "materialized");
        assert_eq!(promo["goals"][0]["id"], objective.id);
        assert!(!promo["tasks"].as_array().unwrap().is_empty());
        let persisted = payload(
            call(
                &engine,
                &json!({
                    "name": "design_query",
                    "arguments": { "view": "promotion", "id": id }
                }),
            )
            .unwrap(),
        );
        assert_eq!(persisted["goals"][0]["id"], objective.id);
        assert_eq!(persisted["tasks"], promo["tasks"]);
        assert_eq!(persisted["constraints"], promo["constraints"]);
        assert!(board(&engine).as_array().unwrap().is_empty());
        let history = payload(
            call(
                &engine,
                &json!({ "name": "design_query", "arguments": { "view": "history", "id": id } }),
            )
            .unwrap(),
        );
        assert_eq!(history.as_array().unwrap().len(), 1);
    }

    #[test]
    fn accepting_your_own_design_is_refused_at_the_tool_boundary() {
        let engine = engine();
        let id = register(&engine, "docs/adr/0099-self.md", "Self");
        let err = decide(&engine, &id, "accept", "planner").unwrap_err();
        assert!(err.contains("own design"));
    }

    /// The ADR-0051 pair, through the one tool that now carries both.
    ///
    /// These guards are the reason the cluster could not simply be deleted:
    /// `attribute` and `reopen` take deliberately disjoint sets of rows, and
    /// collapsing them behind one name is only safe if each still refuses what
    /// it always refused. Attribution must not overwrite a recorded name, and
    /// reopening must defer to a recorded decision.
    #[test]
    fn attribution_and_reopening_still_refuse_what_their_own_names_refused() {
        let engine = engine();
        let id = register(&engine, "docs/adr/0051-attribution.md", "Attribution");
        decide(&engine, &id, "accept", "reviewer").unwrap();

        // Decided by a human already: attribution does not rewrite a recorded act.
        let err = decide(&engine, &id, "attribute", "someone-else").unwrap_err();
        assert!(
            !err.is_empty(),
            "attributing over a recorded decider must refuse"
        );

        // And reopening is refused for a row that carries a decider — it is not
        // an undo.
        let err = call(
            &engine,
            &json!({ "name": "design_decide", "arguments": { "id": id, "decision": "reopen" } }),
        )
        .unwrap_err();
        assert!(!err.is_empty(), "reopening a decided design must refuse");
    }

    #[test]
    fn a_decision_that_needs_a_reason_says_which_decision_wanted_it() {
        let engine = engine();
        let id = register(&engine, "docs/adr/0042-orphan.md", "Orphan");
        let err = call(
            &engine,
            &json!({
                "name": "design_decide",
                "arguments": { "id": id, "decision": "retire", "human": "reviewer" }
            }),
        )
        .unwrap_err();
        assert!(err.contains("retire"), "the refusal must name the decision");
        assert!(err.contains("reason"), "the refusal must name the argument");

        let err = call(
            &engine,
            &json!({
                "name": "design_decide",
                "arguments": { "id": id, "decision": "demote", "human": "reviewer" }
            }),
        )
        .unwrap_err();
        assert!(err.contains("Accepted: accept, reject"), "{err}");
    }

    #[test]
    fn promote_registers_mandated_constraints_through_the_tool() {
        let engine = engine();
        let id = register(&engine, "docs/adr/0030-typed.md", "Typed errors");
        decide(&engine, &id, "accept", "reviewer").unwrap();
        let objective = engine
            .define_goal(GoalKind::Objective, "Type the errors", "do it", None)
            .unwrap();
        let mut plan = payload(
            call(
                &engine,
                &json!({
                    "name": "design_promote",
                    "arguments": { "id": id, "step": "plan", "objective_goal_id": objective.id }
                }),
            )
            .unwrap(),
        );
        plan["constraints"] = json!([
            { "kind": "constraint", "title": "No unwrap", "statement": "no unwrap in prod" }
        ]);
        let promo = payload(
            call(
                &engine,
                &json!({
                    "name": "design_promote",
                    "arguments": { "id": id, "step": "materialize", "plan": plan }
                }),
            )
            .unwrap(),
        );
        let constraints = promo["constraints"].as_array().unwrap();
        assert_eq!(constraints.len(), 1);
        assert_eq!(constraints[0]["kind"], "constraint");
    }

    #[test]
    fn revising_a_promotion_links_existing_work_and_preserves_history() {
        let engine = engine();
        let goal = engine
            .define_goal(GoalKind::Objective, "Existing work", "reuse it", None)
            .unwrap();
        let existing = engine
            .create_task(&goal.id, "Authoritative task", "done")
            .unwrap();
        let id = register(&engine, "docs/adr/0105-repair.md", "Repair");
        decide(&engine, &id, "accept", "reviewer").unwrap();
        let create_plan = payload(
            call(
                &engine,
                &json!({
                    "name": "design_promote",
                    "arguments": { "id": id, "step": "plan", "objective_goal_id": goal.id }
                }),
            )
            .unwrap(),
        );
        call(
            &engine,
            &json!({
                "name": "design_promote",
                "arguments": { "id": id, "step": "materialize", "plan": create_plan }
            }),
        )
        .unwrap();

        let revised = payload(
            call(
                &engine,
                &json!({
                    "name": "design_promote",
                    "arguments": {
                        "id": id,
                        "step": "revise",
                        "human": "second-reviewer",
                        "plan": {
                            "mode": "link",
                            "task_ids": [existing.id],
                            "rationale": "The implementation task already existed"
                        }
                    }
                }),
            )
            .unwrap(),
        );
        assert_eq!(revised["revision"], 2);
        assert_eq!(revised["tasks"][0]["id"], existing.id);
        let history = payload(
            call(
                &engine,
                &json!({ "name": "design_query", "arguments": { "view": "history", "id": id } }),
            )
            .unwrap(),
        );
        assert_eq!(history.as_array().unwrap().len(), 2);
        assert_eq!(history[1]["plan"]["mode"], "link");
    }

    #[test]
    fn reconciling_a_batch_is_idempotent_and_never_creates_tasks() {
        let engine = engine();
        let arguments = json!({
            "designs": [
                {
                    "adr_path": "docs/adr/0026-constitution.md",
                    "title": "Constitution",
                    "summary": "review governance",
                    "status": "proposed",
                    "proposed_by": "workspace-sensor"
                },
                {
                    "adr_path": "docs/adr/0001-historical.md",
                    "title": "Historical",
                    "status": "accepted"
                },
                {
                    "adr_path": "docs/adr/0002-rejected.md",
                    "title": "Rejected",
                    "status": "rejected"
                }
            ]
        });

        let first = payload(
            call(
                &engine,
                &json!({ "name": "design_register", "arguments": arguments }),
            )
            .unwrap(),
        );
        let retry = payload(
            call(
                &engine,
                &json!({ "name": "design_register", "arguments": arguments }),
            )
            .unwrap(),
        );
        assert_eq!(first, retry);
        assert_eq!(first.as_array().unwrap().len(), 3);
        assert!(first
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["promotion_status"] == "not_required"));
        assert!(engine.next_task().unwrap().is_none());

        let board = board(&engine);
        assert_eq!(board.as_array().unwrap().len(), 1);
        assert_eq!(board[0]["id"], "design:0026-constitution");
    }

    #[test]
    fn registering_one_design_and_a_batch_at_once_is_refused() {
        let engine = engine();
        let err = call(
            &engine,
            &json!({
                "name": "design_register",
                "arguments": {
                    "adr_path": "docs/adr/0001-both.md",
                    "title": "Both",
                    "designs": [],
                    "agent": "planner"
                }
            }),
        )
        .unwrap_err();
        assert!(err.contains("not both"), "{err}");
    }

    /// A deprecation that teaches: the old name works and says what to call.
    #[test]
    fn every_old_name_still_answers_and_names_its_replacement() {
        let engine = engine();
        let answer = call(
            &engine,
            &json!({
                "name": "register_design",
                "arguments": {
                    "adr_path": "docs/adr/0059-train.md",
                    "title": "Train",
                    "agent": "planner"
                }
            }),
        )
        .unwrap();
        // The payload a caller already parses is untouched...
        assert_eq!(payload(answer.clone())["status"], "proposed");
        // ...and the lesson rides alongside it.
        let lesson = answer["content"][1]["text"].as_str().unwrap().to_string();
        assert!(lesson.contains("register_design is deprecated"), "{lesson}");
        assert!(lesson.contains("Call design_register instead"), "{lesson}");

        let id = payload(answer)["id"].as_str().unwrap().to_string();
        let accepted = call(
            &engine,
            &json!({ "name": "accept_design", "arguments": { "id": id, "human": "reviewer" } }),
        )
        .unwrap();
        assert_eq!(payload(accepted.clone())["status"], "accepted");
        let lesson = accepted["content"][1]["text"].as_str().unwrap().to_string();
        assert!(
            lesson.contains("design_decide with decision=\"accept\""),
            "{lesson}"
        );
        assert!(lesson.contains("ADR-0059"), "{lesson}");
    }

    /// Every deprecated name points at a verb the server actually advertises,
    /// and no deprecated name is advertised itself — a rename table that drifts
    /// from the tool list teaches a call that does not exist.
    #[test]
    fn the_rename_table_points_only_at_advertised_verbs() {
        let advertised: Vec<String> = list()
            .iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_string))
            .collect();
        for renamed in RENAMED.iter() {
            assert!(
                advertised.iter().any(|name| name == renamed.new),
                "{} points at unadvertised {}",
                renamed.old,
                renamed.new
            );
            assert!(
                !advertised.iter().any(|name| name == renamed.old),
                "{} is deprecated but still advertised",
                renamed.old
            );
        }
        assert_eq!(
            advertised
                .iter()
                .filter(|name| name.starts_with("design_"))
                .count(),
            4,
            "the design cluster is four verbs"
        );
    }
}

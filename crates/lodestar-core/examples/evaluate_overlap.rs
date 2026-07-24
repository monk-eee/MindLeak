//! Deterministic two-agent pre-flight overlap evaluation (ADR-0024).

use std::error::Error;

use lodestar_core::{GoalKind, Lodestar, Task, TaskScope, TaskStatus};
use mindleak_core::{now_unix, Edge, MindLeak, Node, NodeType, RelationType};
use serde_json::{json, Value};

const PATH: &str = "src/lib.rs";
const PATH_GLOB: &str = "src/**/*.rs";
const SYMBOL: &str = "symbol:src/lib.rs:run";
const LEASE_SECS: i64 = 300;
const STALE_AGE_HOURS: i64 = 14 * 24;

type EvaluationResult<T> = Result<T, Box<dyn Error>>;

fn main() -> EvaluationResult<()> {
    println!("{}", serde_json::to_string_pretty(&evaluate()?)?);
    Ok(())
}

fn evaluate() -> EvaluationResult<Value> {
    let evaluated_at = now_unix();
    let control = control_arm()?;
    let aware = overlap_aware_arm(evaluated_at)?;
    Ok(json!({
        "schema_version": 1,
        "scenario": "two_agent_preflight_overlap",
        "evaluated_at": evaluated_at,
        "fixture": {
            "requested_paths": [PATH],
            "requested_symbols": [SYMBOL],
            "declared_path_globs": [PATH_GLOB],
            "agents": ["alice", "bob"],
            "stale_age_hours": STALE_AGE_HOURS
        },
        "control": control,
        "overlap_aware": aware
    }))
}

fn control_arm() -> EvaluationResult<Value> {
    let (engine, first, second) = setup_tasks("Blind control")?;
    let scope = declared_scope();
    let first_claimed = engine.claim_task_with_scope(&first.id, "alice", LEASE_SECS, &scope)?;
    let second_claimed = engine.claim_task_with_scope(&second.id, "bob", LEASE_SECS, &scope)?;
    let concurrent_claims = claimed_count(&engine)?;
    Ok(json!({
        "preflight_check_run": false,
        "first_claimed": first_claimed,
        "second_claimed": second_claimed,
        "concurrent_claims": concurrent_claims,
        "same_path_concurrent_ownership_risk": first_claimed && second_claimed && concurrent_claims == 2
    }))
}

fn overlap_aware_arm(now: i64) -> EvaluationResult<Value> {
    let (engine, first, second) = setup_tasks("Overlap-aware")?;
    let scope = declared_scope();
    let first_claimed = engine.claim_task_with_scope(&first.id, "alice", LEASE_SECS, &scope)?;
    let state_before = task_state(&engine, &first, &second)?;
    let claims = engine.check_claim_overlap(&requested_scope(), Some(&second.id))?;
    let state_after = task_state(&engine, &first, &second)?;

    let active_graph = footprint_graph(now)?;
    let graph_counts_before = active_graph.store().counts(now)?;
    let footprints = active_graph.check_overlap(&[PATH.to_string()], &[], Some("bob"))?;
    let graph_counts_after = active_graph.store().counts(now)?;

    let stale_graph = footprint_graph(now - STALE_AGE_HOURS * 3600)?;
    let stale_footprints = stale_graph.check_overlap(&[PATH.to_string()], &[], Some("bob"))?;
    let should_steer = !claims.is_empty() || !footprints.is_empty();
    let handoff_applied = should_steer && engine.block_task(&second.id, Some(first.id.clone()))?;
    let second_claimed_after_steer = engine.claim_task(&second.id, "bob", LEASE_SECS)?;
    let second_after_steer = engine.store().get_task(&second.id)?.unwrap();
    let concurrent_claims_after_steer = claimed_count(&engine)?;

    Ok(json!({
        "first_claimed": first_claimed,
        "claim_overlaps": claims,
        "footprint_overlaps": footprints,
        "decay_control": {
            "seeded_age_hours": STALE_AGE_HOURS,
            "footprint_overlaps": stale_footprints
        },
        "checks_read_only": state_before == state_after && graph_counts_before == graph_counts_after,
        "task_state_unchanged_by_check": state_before == state_after,
        "graph_counts_before": graph_counts_before,
        "graph_counts_after": graph_counts_after,
        "steer": {
            "action": "blocked_by_handoff",
            "applied": handoff_applied,
            "second_status": second_after_steer.status.as_str(),
            "second_blocked_by": second_after_steer.blocked_by,
            "second_claimed_after_steer": second_claimed_after_steer,
            "concurrent_claims_after_steer": concurrent_claims_after_steer,
            "same_path_concurrent_ownership_risk": concurrent_claims_after_steer > 1
        }
    }))
}

fn setup_tasks(label: &str) -> EvaluationResult<(Lodestar, Task, Task)> {
    let engine = Lodestar::open_in_memory()?;
    let goal = engine.define_goal(
        GoalKind::Objective,
        label,
        "Coordinate two different tasks that touch the same path",
        None,
    )?;
    let first = engine.create_task(&goal.id, "Alice work", "path updated")?;
    let second = engine.create_task(&goal.id, "Bob work", "path updated")?;
    Ok((engine, first, second))
}

fn footprint_graph(observed_at: i64) -> EvaluationResult<MindLeak> {
    let engine = MindLeak::open_in_memory()?;
    for (id, kind) in [
        ("agent:alice", NodeType::Agent),
        ("execution:alice", NodeType::Execution),
        ("artifact:src/lib.rs", NodeType::Artifact),
    ] {
        engine
            .store()
            .upsert_node(&Node::new(id, kind, id, observed_at))?;
    }
    engine.store().upsert_edge(&Edge::new(
        "agent:alice",
        "execution:alice",
        RelationType::Observed,
        observed_at,
    ))?;
    engine.store().upsert_edge(&Edge::new(
        "execution:alice",
        "artifact:src/lib.rs",
        RelationType::Modified,
        observed_at,
    ))?;
    Ok(engine)
}

fn declared_scope() -> TaskScope {
    TaskScope {
        paths: vec![PATH_GLOB.to_string()],
        symbols: vec![SYMBOL.to_string()],
    }
}

fn requested_scope() -> TaskScope {
    TaskScope {
        paths: vec![PATH.to_string()],
        symbols: vec![SYMBOL.to_string()],
    }
}

fn task_state(engine: &Lodestar, first: &Task, second: &Task) -> EvaluationResult<Value> {
    Ok(json!({
        "first": engine.store().get_task(&first.id)?,
        "second": engine.store().get_task(&second.id)?,
        "first_scope": engine.task_scope(&first.id)?,
        "second_scope": engine.task_scope(&second.id)?
    }))
}

fn claimed_count(engine: &Lodestar) -> EvaluationResult<usize> {
    Ok(engine
        .board(true)?
        .iter()
        .filter(|task| task.status == TaskStatus::Claimed)
        .count())
}

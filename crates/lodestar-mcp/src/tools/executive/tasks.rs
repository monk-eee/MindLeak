//! Read-and-summarize helpers for `task_query` views: existing work, task
//! completion, and the coordination board snapshot.

use super::super::conformance::{delivered_completion_conformance, parse_evidence};
use super::super::{bool_arg, ok, opt_str, str_array};
use super::claim::attach_lease_warning;
use super::constants::TASK_PREVIEW_LIMIT;
use lodestar_core::{now_unix, ConformanceCheckReference, Lodestar, Task, TaskStatus};
use serde_json::{json, Value};

/// Newest-first truncation of a task list too long to send whole. The exact
/// count of what was cut is computed by the caller before this runs, so
/// nothing about "how much" is lost — only the list itself is capped.
pub(super) fn bounded_by_recency(mut tasks: Vec<Task>, limit: usize) -> (Vec<Task>, bool) {
    tasks.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    let truncated = tasks.len() > limit;
    tasks.truncate(limit);
    (tasks, truncated)
}

/// Has this already been done? Terminal work is included on purpose: a task
/// that is already finished is the most useful answer, and the one the board
/// hides (ADR-0015).
pub(super) fn existing_work(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let goal_id = opt_str(args, "goal_id");
    let paths = str_array(args, "paths");
    if goal_id.is_none() && paths.is_empty() {
        return Err(
            "\"existing_work\" needs a goal_id or paths; asking about nothing answers nothing"
                .to_string(),
        );
    }
    let found = engine
        .existing_work(goal_id.as_deref(), &paths)
        .map_err(|e| e.to_string())?;
    let total = found.len();
    let finished = found
        .iter()
        .filter(|task| task.status == TaskStatus::Done)
        .count();
    let (bounded, truncated) = bounded_by_recency(found, TASK_PREVIEW_LIMIT);
    let rows: Vec<Value> = bounded
        .iter()
        .map(|task| {
            json!({
                "task_id": task.id,
                "title": task.title,
                "status": task.status.as_str(),
                "goal_id": task.goal_id,
                "owner": task.owner,
            })
        })
        .collect();
    ok(&json!({
        "count": total,
        "already_done": finished,
        "work": rows,
        "work_truncated": truncated,
    }))
}

pub(super) fn complete(engine: &Lodestar, task_id: &str, args: &Value) -> Result<Value, String> {
    let evidence = parse_evidence(args)?;
    let check = args
        .get("check")
        .cloned()
        .ok_or_else(|| {
            "\"complete\" requires check: the id/token reference or full check returned by check_conformance."
                .to_string()
        })?;
    let learned = opt_str(args, "learned");
    let completion = if check.get("findings").is_some() {
        let check = serde_json::from_value(check)
            .map_err(|error| format!("invalid full conformance check: {error}"))?;
        engine.complete_task(
            task_id,
            opt_str(args, "agent").unwrap_or_default().as_str(),
            &evidence,
            &check,
            learned.as_deref(),
        )
    } else {
        let check: ConformanceCheckReference = serde_json::from_value(check)
            .map_err(|error| format!("invalid compact conformance check: {error}"))?;
        engine.complete_task_with_check_reference(
            task_id,
            opt_str(args, "agent").unwrap_or_default().as_str(),
            &evidence,
            &check,
            learned.as_deref(),
        )
    }
    .map_err(|e| e.to_string())?;
    let mut response = json!({
        "completed": completion.completed,
        "conformance": delivered_completion_conformance(&completion.conformance),
    });
    // The omission is reported, never blocked (ADR-0053). Most tasks
    // teach nothing; a gate would produce a column of "n/a" and the gap
    // would stay invisible. Naming it is what makes it measurable.
    match &completion.learned {
        Some(id) => {
            response["learned"] = json!(id);
        }
        None => {
            response["learned_omitted"] = json!(
                "no conclusion recorded; pass `learned` with what the next agent should know, or nothing if this taught nothing"
            );
        }
    }
    // The one place an agent reliably discovers a dead lease, and until
    // now it discovered only that completion failed. If any claim of
    // theirs has lapsed, say so here with what can and cannot be done
    // about it -- renewing does not repair a holed window.
    attach_lease_warning(engine, args, &mut response)?;
    ok(&response)
}
/// The coordination snapshot: every task with the facts a reader would
/// otherwise have to go and derive — declared scope, the receipt it closed on,
/// whether its evidence window is continuous, and whether the claim is actually
/// being held rather than merely recorded.
pub(super) fn board(engine: &Lodestar, args: &Value) -> Result<Value, String> {
    let tasks = engine
        .board(bool_arg(args, "include_terminal", true))
        .map_err(|e| e.to_string())?;
    let mut rows = Vec::with_capacity(tasks.len());
    for task in tasks {
        let scope = engine.task_scope(&task.id).map_err(|e| e.to_string())?;
        // The receipt the task closed on, beside the status rather than
        // one query away. A `done` row says nothing about whether its
        // evidence ever affirmed the work, and most of them did not.
        let receipt = engine.task_receipt(&task.id).map_err(|e| e.to_string())?;
        // Evidence-window continuity, derived from the task log rather
        // than read off the row (ADR-0064 d5). It rides here because a
        // discontinuous window is why a task cannot close itself, and
        // that is a fact about the row a reader should not have to go
        // looking for.
        let window = engine.claim_window(&task.id).map_err(|e| e.to_string())?;
        // Whether the claim is actually being held, beside the status
        // rather than inferred from a timestamp. `claimed` alone read as
        // work in progress: measured once at 36 claimed rows of which 4
        // had a live lease, and finding that out took a bespoke script.
        let lease_state = match (task.status, task.lease_expires_at) {
            (TaskStatus::Claimed, Some(expires)) if expires >= now_unix() => Some("live"),
            (TaskStatus::Claimed, _) => Some("lapsed"),
            _ => None,
        };
        let mut row = serde_json::to_value(task).map_err(|e| e.to_string())?;
        let object = row
            .as_object_mut()
            .ok_or_else(|| "task did not serialize as an object".to_string())?;
        object.insert("scope".to_string(), json!(scope));
        object.insert("claim_window".to_string(), json!(window));
        if let Some(state) = lease_state {
            object.insert("lease_state".to_string(), json!(state));
        }
        if let Some(receipt) = receipt {
            object.insert("receipt".to_string(), json!(receipt));
        }
        rows.push(row);
    }
    ok(&rows)
}

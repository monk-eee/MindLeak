//! Claim lifecycle: the compare-and-swap claim itself, why a claim was lost,
//! and the lease/attention nudges attached to responses.

use super::{i64_arg, ok, opt_str, str_array};
use lodestar_core::{now_unix, Lodestar, TaskStatus};
use serde_json::{json, Value};

/// The claim itself: a compare-and-swap, plus everything the losing side needs.
pub(super) fn claim(engine: &Lodestar, task_id: &str, args: &Value) -> Result<Value, String> {
    let paths = args.get("paths").map(|_| str_array(args, "paths"));
    let symbols = args.get("symbols").map(|_| str_array(args, "symbols"));
    let agent = opt_str(args, "agent").unwrap_or_default();
    let won = engine
        .claim_task_with_partial_scope(
            task_id,
            agent.as_str(),
            i64_arg(args, "lease_secs", 300),
            paths.as_deref(),
            symbols.as_deref(),
        )
        .map_err(|e| e.to_string())?;
    let mut response = json!({ "won": won, "governing": [] });
    // The claim opens the window that completion later validates evidence
    // against: evidence starting before `claim_started_at` is refused as
    // "outside the live claim". Returning the window here is what makes that
    // constructible — without it an agent has to guess a `started_at`, and a
    // wrong guess reads as a policy refusal rather than a missing accessor
    // (ADR-0060).
    if won {
        let scope = engine.task_scope(task_id).map_err(|e| e.to_string())?;
        // Coverage rides on the claim rather than on a verb of its own: the
        // claim is already where a task says what it will touch, and a
        // same-owner re-claim keeps the evidence window open, so an agent that
        // learns mid-change which goals actually govern its files can say so
        // without opening a window that cannot own its own work (ADR-0041).
        let also_serves = str_array(args, "also_serves");
        if !also_serves.is_empty() {
            let covered = engine
                .declare_coverage(task_id, agent.as_str(), &also_serves)
                .map_err(|e| e.to_string())?;
            if let Some(obj) = response.as_object_mut() {
                obj.insert("also_serves".to_string(), json!(covered));
            }
        }
        // Coverage must be recorded before the scope is classified: a caller
        // can supply the correction on this same claim, and reporting stale
        // review advice after accepting it would send the agent in a loop.
        let governing = engine
            .governing_clauses_for_task(task_id)
            .map_err(|e| e.to_string())?;
        let scope_advice = engine
            .advice_for_task_scope(task_id)
            .map_err(|e| e.to_string())?;
        if let Some(obj) = response.as_object_mut() {
            obj.insert("governing".to_string(), json!(governing));
            if let Some(advice) = scope_advice {
                obj.insert("scope_advice".to_string(), json!(advice));
            }
        }
        if let Some(task) = engine
            .store()
            .get_task(task_id)
            .map_err(|e| e.to_string())?
        {
            if let Some(obj) = response.as_object_mut() {
                obj.insert("claim_started_at".to_string(), json!(task.claim_started_at));
                obj.insert("lease_expires_at".to_string(), json!(task.lease_expires_at));
                // The branch this claim's window is being done on, confirmed
                // back at the decision point (ADR-0035 d5). Joined at claim time
                // from what the session told open_session, so it is null when
                // none was declared rather than guessed.
                obj.insert("branch".to_string(), json!(task.branch));
                // ADR-0099: a scope-less claim is invisible to check_overlap and
                // view="drafts" alike, since both key on declared scope. This
                // check does not: it is title-based and runs regardless of what
                // scope (if any) was just declared, reporting another live task
                // under this same goal sharing this exact title -- the same
                // (title, goal_id) collision existing_work already answers, now
                // asked automatically. A different-goal same-title claim is
                // legitimate (ADR-0015) and stays this check's non-concern;
                // that signal is already task_create's same_title_under_other_goals.
                // Never blocks -- only makes the collision visible instead of
                // relying on the claimant to think to run existing_work.
                let twin = engine
                    .live_task_titled(&task.goal_id, &task.title)
                    .map_err(|e| e.to_string())?
                    .filter(|existing| existing.id != task.id);
                if let Some(twin) = twin {
                    obj.insert(
                        "title_twin".to_string(),
                        json!({
                            "task_id": twin.id,
                            "goal_id": twin.goal_id,
                            "owner": twin.owner,
                            "branch": twin.branch,
                        }),
                    );
                }
                if !scope.paths.is_empty() || !scope.symbols.is_empty() {
                    obj.insert(
                        "memory_preflight".to_string(),
                        json!({
                            "plane": "mindleak",
                            "tool": "check_overlap",
                            "when": "before the first edit",
                            "advisory": true,
                            "arguments": {
                                "paths": scope.paths,
                                "symbols": scope.symbols,
                            },
                            "reason": "ADR-0066 retrieval is a separate cross-plane read; this claim has not performed it.",
                        }),
                    );
                }
            }
        }
    } else {
        // A lost claim used to be a bare `won: false`. Every reason it
        // can fail is knowable from the row the compare-and-swap just
        // missed, and they call for opposite responses: wait for a live
        // lease, pick different work if it is finished, unblock a
        // blocker, rebuild a stale binary. Collapsing them into one
        // boolean is why `scripts/claim-gate.mjs` exists at all — a
        // whole diagnostic written to guess, after the fact, at
        // something the plane knew at the time.
        let task = engine
            .store()
            .get_task(task_id)
            .map_err(|e| e.to_string())?;
        if let Some(obj) = response.as_object_mut() {
            obj.insert(
                "reason".to_string(),
                json!(lost_claim_reason(task.as_ref(), &agent, now_unix())),
            );
            if let Some(task) = task.as_ref() {
                obj.insert("status".to_string(), json!(task.status.as_str()));
                obj.insert("owner".to_string(), json!(task.owner));
                // Not just who holds it but the branch they hold it on
                // (ADR-0035 d5): the fact a colliding agent needs to tell a
                // merge risk from the same work twice. Pinned to the owner's
                // window at claim time, null when they declared no branch.
                obj.insert("owner_branch".to_string(), json!(task.branch));
                obj.insert("lease_expires_at".to_string(), json!(task.lease_expires_at));
                obj.insert("blocked_by".to_string(), json!(task.blocked_by));
            }
        }
    }
    attach_owner_attention(engine, args, &mut response)?;
    ok(&response)
}

/// Attach any questions addressed to this agent to a response (ADR-0046).
///
/// Delivered through calls the agent already makes — `claim_task` at pickup and
/// Why a compare-and-swap claim missed, in words the caller can act on.
///
/// Every one of these was already knowable from the row; `claim_task` simply
/// returned `false` and let the agent guess. They call for opposite responses —
/// wait, pick different work, unblock a predecessor, rebuild a binary — so one
/// boolean covering all of them is not terse, it is unusable. `claim-gate.mjs`
/// is a whole diagnostic written to reconstruct, after the fact, something the
/// plane knew at the moment it refused.
pub(super) fn lost_claim_reason(
    task: Option<&lodestar_core::Task>,
    agent: &str,
    now: i64,
) -> String {
    let Some(task) = task else {
        return "no such task".to_string();
    };
    if let Some(blocker) = task.blocked_by.as_deref() {
        return format!("blocked by {blocker}; it must complete aligned first");
    }
    match task.status {
        TaskStatus::Done | TaskStatus::Abandoned => {
            format!(
                "already {}; there is nothing to claim",
                task.status.as_str()
            )
        }
        _ => match task.owner.as_deref() {
            // ADR-0054: a server built before session identities resolves a
            // different id shape than the migrated ledger holds, so an agent is
            // refused its own claim. Nothing in `won: false` hinted at that, and
            // re-claiming never helps — it cost a long hunt once already.
            Some(owner)
                if !owner.starts_with("session:v1:") && agent.starts_with("session:v1:") =>
            {
                format!(
                    "held under a pre-session identity ({owner}); this is a stale server binary \
                     (ADR-0054), not a live claim. Rebuild and reinstall — re-claiming will not help"
                )
            }
            Some(owner) if owner != agent => match task.lease_expires_at {
                Some(expires) if expires > now => format!(
                    "held by {owner} for another {}s; task_claim with step=\"recover\" can take it over with a reason",
                    expires - now
                ),
                _ => format!(
            "held by {owner} with a lapsed lease; use task_claim with step=\"recover\" to take it"
        ),
            },
            _ => format!(
                "status {} does not accept a claim right now",
                task.status.as_str()
            ),
        },
    }
}

/// `renew_lease` as the heartbeat — rather than by a new obligation to poll.
/// A capability nobody remembers to call is adopted at the rate we measured for
/// the whole intent plane: zero. The heartbeat is the important one, because a
/// question usually arrives *during* the work rather than before it.
///
/// Absent when nothing is waiting: no key, no empty array, nothing for a caller
/// to interpret. A reader must never have to tell "no questions" apart from
/// "this server does not report questions".
/// How close a lease may get to expiry before the agent is told.
///
/// Five minutes is the default lease and `cargo test --all` alone can outlast
/// it, so a lapse is not an exceptional event -- it is the normal outcome of
/// doing a normal thing. Ninety seconds is enough to call `renew_lease` and
/// short enough not to cry wolf on every call.
const LEASE_WARNING_SECS: i64 = 90;

/// Tell an agent its lease is about to die, while it can still do something.
///
/// A lapse is currently silent. The agent finds out at `complete_task`, which
/// is far too late: closing a lapsed claim means re-claiming it, re-claiming
/// records the lapse, and conformance then refuses to certify across the hole
/// (ADR-0048). The cost of missing the deadline is unrecoverable, and the only
/// warning arrived after the deadline had passed.
///
/// This is the complement to the heartbeat (ADR-0052), which renews the lease
/// on six tools an agent already calls. Those six cover the common path; this
/// covers everything else by making the deadline visible rather than assuming
/// it will be met.
pub(super) fn attach_lease_warning(
    engine: &Lodestar,
    args: &Value,
    response: &mut Value,
) -> Result<(), String> {
    let agent = opt_str(args, "agent").unwrap_or_default();
    if agent.is_empty() {
        return Ok(());
    }
    let now = now_unix();
    let mine: Vec<_> = engine
        .board(false)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|task| {
            task.status == TaskStatus::Claimed && task.owner.as_deref() == Some(agent.as_str())
        })
        .collect();

    let expiring: Vec<Value> = mine
        .iter()
        .filter(|task| {
            task.lease_expires_at
                .is_some_and(|at| at > now && at - now <= LEASE_WARNING_SECS)
        })
        .map(|task| {
            json!({
                "task_id": task.id,
                "seconds_left": task.lease_expires_at.unwrap_or(now) - now,
            })
        })
        .collect();

    // An already-lapsed lease is the more urgent signal, not a less urgent one.
    // The damage is done, but the sooner the agent knows, the sooner it stops
    // producing work it can never certify -- and the alternative is finding out
    // at `complete_task`, having built a whole change on a dead claim.
    let lapsed: Vec<Value> = mine
        .iter()
        .filter(|task| task.lease_expires_at.is_some_and(|at| at <= now))
        .map(|task| {
            json!({
                "task_id": task.id,
                "seconds_ago": now - task.lease_expires_at.unwrap_or(now),
            })
        })
        .collect();

    if expiring.is_empty() && lapsed.is_empty() {
        return Ok(());
    }
    if let Some(object) = response.as_object_mut() {
        if !expiring.is_empty() {
            object.insert("lease_expiring".to_string(), json!(expiring));
            object.insert(
                "lease_advice".to_string(),
                json!(
                    "task_claim with step=\"renew\" now. A lapsed claim cannot be completed afterwards: \
                     re-claiming records the lapse and conformance refuses to certify \
                     across the hole (ADR-0048)."
                ),
            );
        }
        if !lapsed.is_empty() {
            object.insert("lease_lapsed".to_string(), json!(lapsed));
            object.insert(
                "lapsed_advice".to_string(),
                json!(
                    "This claim can no longer be completed by an agent. Renewing will not \
                     repair it -- the evidence window already has a hole, and conformance \
                     refuses to certify across one (ADR-0048). Stop and get a human to \
                     confirm it; `make stranded-report` names the likely commit."
                ),
            );
        }
    }
    Ok(())
}

pub(in crate::tools) fn attach_owner_attention(
    engine: &Lodestar,
    args: &Value,
    response: &mut Value,
) -> Result<(), String> {
    let agent = args
        .get("resolved_agent")
        .or_else(|| args.get("agent"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if agent.is_empty() {
        return Ok(());
    }
    let waiting = engine.pending_questions(agent).map_err(|e| e.to_string())?;
    if let Some(object) = response.as_object_mut() {
        if !waiting.is_empty() {
            object.insert(
                "waiting_on_you".to_string(),
                serde_json::to_value(&waiting).map_err(|e| e.to_string())?,
            );
        }

        let mut paused = Vec::new();
        for task in engine.board(false).map_err(|e| e.to_string())? {
            if task.status != TaskStatus::Paused || task.owner.as_deref() != Some(agent) {
                continue;
            }
            let reason = engine
                .task_qa(&task.id)
                .map_err(|e| e.to_string())?
                .into_iter()
                .rev()
                .find(|entry| {
                    entry.kind == "note"
                        && task
                            .parked_at
                            .is_some_and(|parked_at| entry.created_at == parked_at)
                })
                .map(|entry| entry.body);
            paused.push(json!({
                "task_id": task.id,
                "title": task.title,
                "parked_at": task.parked_at,
                "reason": reason,
            }));
        }
        if !paused.is_empty() {
            object.insert("paused_by_you".to_string(), json!(paused));
            object.insert(
                "paused_advice".to_string(),
                json!("Call task_transition with to=\"resume\" for paused work you are continuing. `needs_input` is different: answer its question instead, with to=\"answer\"."),
            );
        }
    }
    Ok(())
}

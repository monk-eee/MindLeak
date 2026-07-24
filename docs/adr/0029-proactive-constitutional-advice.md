# ADR-0029 - Proactive constitutional advice (ask-before-act)

- **Status:** Proposed
- **Date:** 2026-07-24
- **Related:** [ADR-0004](0004-intent-plane-spec-brain.md) (Intent Plane),
  [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed conformance),
  [ADR-0025](0025-authoritative-checked-conformance.md) (authoritative checked
  completion), [ADR-0026](0026-constitutional-policy-over-mechanistic-ratchets.md)
  (constitutional policy), [ADR-0024](0024-preflight-overlap-detection.md)
  (overlap preflight — a *different* preflight)

## Context

Lodestar can hold a durable Constitution (objectives, constraints, invariants)
and, at `complete_task`, judge an agent's actual work against the clauses that
govern the code it changed. That enforcement is **retrospective**: the verdict
arrives *after* the work is done. The only forward-looking signal today is a
passive nudge — the `define_goal` tool description says "read the constitution
before acting", and `get_constitution` returns the whole set as an
undifferentiated dump. `governing_goals` can answer "which clauses govern this
node", but only if an agent thinks to call it.

Nothing **drives** an agent to ask, up front, the question a careful contributor
asks first: *"Given this task and the files I am about to touch, what governs me,
and does anything here advise caution, require review, or forbid the change?"*
The resulting failure mode is expensive and common: an agent completes an entire
task, then discovers at `complete_task` that it drifted outside its objective or
breached an invariant — work that must now be unwound. The Constitution is
authoritative but only consulted at the end.

We want the agent to **ask before acting**, against the *specific* work it is
about to do, and to be routed to that question by the workflow rather than by
goodwill.

## Decision

Add a **proactive, forward-looking, evidence-free, state-free constitutional
advisory** and make consulting it part of the claim ritual. Three parts:

1. **An advisory query (working name `advise`).** Input: a `task_id` plus the
   intended changed `node_ids` (`artifact:`/`symbol:` ids) and, later, a declared
   workflow scope. Output: the active clauses that govern that intended scope —
   each with its rationale and declared consequence — and a single **proportional
   disposition**: `advise`, `review`, `block`, or `needs_human`. It is the
   proactive sibling of `check_conformance`: it reuses the same clause resolution
   (load active version, find clauses governing the declared nodes, apply each
   clause's consequence, resolve broad principles to `needs_human`) but **takes no
   evidence bundle, records no conformance verdict, and changes no task state**.
   It answers "what would govern this?", not "did this comply?".

2. **Surfacing at coordination points.** `claim_task` and `next_task` responses
   include the clauses governing the task's linked scope, so an agent that picks
   up work is shown "here is what governs this" without a second call. The VS Code
   Intent Board shows the same governing clauses on a claimed task.

3. **Ritual plus backstop.** `AGENTS.md` mandates calling `advise` at claim time
   and before editing any `governed`/`forbid_change` file. The existing
   retrospective conformance at `complete_task` (ADR-0009/0025) remains the teeth:
   skipping the advisory does not avoid the verdict, it only forfeits the early
   warning. Advice never gates the claim itself — coordination stays compare-and-
   swap (ADR-0004); the advisory informs, it does not lock.

The advisory is **fail-open and proportional** (ADR-0026): an absent constitution
yields `needs_human`/advisory guidance, never a pre-emptive `block`; only a
sufficiently specific active clause governing the declared scope can advise a
`block`, and every disposition names the clause and rationale that produced it.
It is **deterministic** — clause resolution needs no model; an optional LLM may
only phrase the rationale, never decide the disposition.

This operationalises SPEC-CONSTITUTION §7.6 ("agents read the constitution before
acting"): it turns a passive dump into an active, scoped, per-action question the
workflow asks on the agent's behalf.

## Boundaries

- **Not overlap preflight.** ADR-0024's preflight detects two agents about to
  touch the same work; this preflight compares *one* agent's intent against
  *policy*. They are complementary and independently useful.
- **Not a lock and not a gate.** The advisory cannot block claiming or editing;
  it advises. Hard enforcement remains the retrospective, evidence-bearing
  conformance record.
- **No new authority.** The advisory invents no clauses and stores nothing; it is
  a read-only projection of the already-adopted Constitution.

## Consequences

- Agents catch drift and forbidden-change risk **before** doing the work, cutting
  wasted effort and rework.
- The Constitution becomes a *consulted* authority at the start of work, not a
  decorative preamble read at the end.
- The change is cheap and safe: read-only, deterministic, no state, no model
  dependency, no change to the authoritative completion protocol.
- It adds one small tool and one claim-time ritual line; the retrospective record
  (ADR-0025) stays the single source of truth for what actually happened.

## Rejected alternatives

- **Rely only on retrospective conformance.** Correct but late: it discovers
  violations after the work, forcing unwind. The advisory is the early warning.
- **Hard-block claiming when advice says `block`.** Turns an advisory into a lock,
  violating ADR-0026 proportionality and the CAS-only coordination model; a stale
  or over-broad clause could freeze legitimate work.
- **Keep `get_constitution` as the only forward signal.** Passive and unscoped;
  an agent must read everything and self-filter, which is exactly what is ignored
  today.
- **Make the advisory LLM-gated.** Breaks the zero-dependency deterministic
  posture; policy resolution must not require a reachable model.

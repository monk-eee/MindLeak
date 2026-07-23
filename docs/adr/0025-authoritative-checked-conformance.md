# ADR-0025 - Authoritative checked conformance

- **Status:** Accepted
- **Date:** 2026-07-23

## Context

ADR-0009 made `check_conformance` and `complete_task` use the same evaluator,
but each call evaluated independently. For normative goals that evaluator invokes
the optional semantic judge. The same immutable evidence therefore preflighted as
`aligned` and immediately completed as `needs_human` when the model returned a
different answer on the second call. The second verdict moved valid work to
`in_review` and left two audit rows that disagreed about one decision.

Retrying the model, caching a response in process memory, or bypassing
conformance would hide the race rather than define which verdict owns the task
transition. The durable conformance audit already provides the right authority
boundary: a stable id, exact evidence, verdict, findings, and check time.

## Decision

Conformance completion is a two-phase checked protocol:

1. `check_conformance(evidence, task_id?)` validates the claim and evidence,
   evaluates structural policy, optional semantic judgment, and advisory
   knowledge exactly once, and appends one conformance audit row.
2. It returns `ConformanceCheck { id, token, verdict, findings }`. The SHA-256
   token covers the audit id, exact evidence, result, current task goal, active
   bindings for changed nodes, and matching active knowledge.
3. `complete_task(task_id, agent, evidence, check)` reloads the audit row and
   verifies its task, evidence, verdict, findings, claim window, and recomputed
   token. It never invokes the semantic judge.
4. A changed evidence bundle or relevant intent/knowledge state makes the check
   stale. Completion rejects it and requires a new check rather than silently
   changing verdict.
5. The checked audit row is the sole durable evidence link controlling the
   atomic task transition. Completion does not append a duplicate audit row.

Only an authoritative `aligned` check reaches `done`; `drift` and
`needs_human` move to `in_review`, and `violation` moves to `blocked`, preserving
ADR-0009's transition table. Reopening review work preserves the audit chain.
The optional model remains off deterministic ingest/query paths.

This decision supersedes ADR-0009 only where it says `complete_task` performs a
fresh conformance evaluation and records another result during transition.

## Consequences

- Preflight and completion cannot disagree for unchanged evidence and intent.
- Clients must call `check_conformance` first and pass its exact result to
  `complete_task`; this is an intentional MCP contract change.
- Relevant state changes are explicit stale-check errors instead of hidden
  time-of-check/time-of-use drift.
- The append-only history contains one authoritative record per check, including
  checks that are never consumed or lead to review.

## Rejected alternatives

- **Rerun until two model answers agree:** nondeterministic, unbounded, and easy
  to game.
- **In-memory verdict cache:** lost on restart and not authoritative across MCP
  processes.
- **Let completion rerun only deterministic checks and trust an unaddressed
  model result:** cannot prove which persisted semantic result was consumed.
- **Make semantic judgment advisory-only in this change:** broader policy change
  that weakens normative-goal enforcement; it requires a separate decision.

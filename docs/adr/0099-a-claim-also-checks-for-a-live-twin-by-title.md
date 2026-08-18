# ADR-0099: A claim also checks for a live twin by title, not only by scope

- Status: Accepted
- Date: 2026-08-18
- Deciders: MindLeak maintainers
- Related: [ADR-0024](0024-preflight-overlap-detection.md) (pre-flight overlap
  detection), [ADR-0055](0055-draft-the-question-decide-nothing.md) (draft the
  question, decide nothing), [ADR-0009](0009-evidence-backed-conformance.md)
  (evidence-backed conformance)

## Context

[ADR-0024](0024-preflight-overlap-detection.md) made scope declaration on a
claim optional — a planning hint, never a lock, deliberately not costed as
mandatory. That was the right call for the cost it was pricing: naming paths and
symbols before you know them precisely.

It has a blind spot ADR-0024 did not anticipate, recorded live in
`gaps.d/scopeless-claim-is-invisible-to-duplicate-work-detection.md`. Two
sessions held the same task title at the same time —
`task:523510b1663f` and `task:b0979f99d856`, both "Implement: ADR-0082:
Ackplane is a standalone federation service." One declared ten paths; the other
declared `paths: [], symbols: []`. `check_overlap` and `view="drafts"`
(ADR-0055) both key entirely on declared scope, so the scope-less claim was
structurally invisible to both — not a missed warning, a check with nothing to
compare. The duplication had time to become real code on two branches,
`feat/ackplane-node-protocol` and `feat/ackplane-node-protocol-isolated`, before
anyone noticed.

`task_query(view="existing_work")` already answers the question that would have
caught this — identical title, identical goal, two live owners is the strongest
duplicate signal the ledger holds — but nothing consults it at claim time. An
agent has to think to run it, about work it does not yet know exists.

The gap fragment correctly refused two tempting fixes: making scope mandatory
(re-prices what ADR-0024 decided declaring a scope should cost), and widening
`check_overlap`'s own scope-keyed signal to fall back on title matching (blurs
what a scope check means and what a title check means, into one tool whose
result an agent can no longer reason about). Both change an existing contract.
This ADR adds a second, independent signal instead of changing either.

## Decision

**A claim always checks for a live twin by `(title, goal_id)`, whether or not it
declares scope.** This is orthogonal to ADR-0024's scope-based overlap check,
not a replacement for it — a claim gets both signals, and each can fire without
the other.

1. **The check runs inside `task_claim(step="claim")`, unconditionally.** Before
   granting a claim, look for another **live-owned** task (`status="claimed"`,
   lease not lapsed) sharing this task's exact title and `goal_id`. This is the
   same query `existing_work` already runs; it is invoked automatically here
   rather than left for an agent to think to run.
2. **A twin does not block the claim.** Consistent with ADR-0024's advisory
   stance: this reports, it does not refuse. The requesting agent decides
   whether to proceed, coordinate, or stand down — the same posture
   `check_overlap` already takes toward scope collisions.
3. **The claim response carries the signal.** A won claim's result gains an
   optional `title_twin` field naming the other task id and owner when one
   exists; absent when there is none. No new tool, no new call an agent must
   remember to make — the information arrives with the claim itself.
4. **This does not touch declared scope.** `paths`/`symbols` remain exactly as
   costed by ADR-0024: optional, advisory, unpriced beyond what an agent chooses
   to declare. A scope-less claim is exactly as cheap to make as before; it is
   only no longer invisible to *this* check, which never looked at scope in the
   first place.
5. **This does not touch `view="drafts"` (ADR-0055).** Drafting a phrased
   question from a scope collision is unchanged. A title-twin signal is a bare
   fact (id + owner), not a phrased question — an agent that sees one can
   compose its own question, or read the twin's thread, exactly as it does
   today for any other collision it discovers.

## Consequences

- Closes the specific hole the gap fragment measured: a scope-less claim can no
  longer be structurally invisible to duplicate-title detection, because this
  check never depended on scope to begin with.
- Two agents legitimately claiming the same title under different goals (a
  real, non-duplicate case — see
  `gaps.d/the-board-hands-one-adr-to-several-agents-at-once.md`'s note on task
  generation producing identical titles under multiple objectives on purpose)
  are unaffected: the check keys on `(title, goal_id)` together, not title
  alone.
- Does not close `gaps.d/rescuing-a-lapsed-lease-can-duplicate-a-published-pr.md`
  — that gap is about a lease that has *already lapsed* by the time a rescuer
  looks, which this check (live-owned twins only) does not cover by design; a
  lapsed twin needs `gh pr list --head <branch> --state all`, not a ledger read,
  because the ledger cannot see GitHub.
- Adds one query to the claim path. `existing_work` is already an indexed
  `(title, goal_id, status)` lookup; this is the same cost `existing_work`
  already pays when an agent remembers to call it, now paid once per claim
  instead of never.

# ADR-0076: Evidence is judged against the window that authorised the work

- Status: Accepted
- Date: 2026-07-31
- Decider: MindLeak maintainer
- Related: [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed
  conformance), [ADR-0030](0030-discrete-per-agent-identity.md) (guarded
  recovery of stranded claims),
  [ADR-0048](0048-a-lapsed-lease-holes-the-window-it-does-not-move-it.md) (a
  lapse holes the window, it does not move it),
  [ADR-0064](0064-the-log-is-the-ledger.md) (the log is the ledger)

## Context

ADR-0009 bounds evidence by a live claim: `check_conformance` refuses a bundle
whose `started_at` precedes `task.claim_started_at`. That rule is what stops an
agent certifying work it never held the task for, and it is not in question.

The problem is which window it compares against. ADR-0030 lets a session recover
a claim stranded under a legacy identity, and recovery necessarily happens
*after* the work it exists to rescue — that is the situation it is for. But
`recover_claim` opens a fresh window at the moment of recovery, so the rescued
work now sits before the only window the task can show.

Every route back to a live claim had the same shape. `claim_task` sets
`claim_started_at` to now for a new owner, `recover_claim` sets it to now, and
`renew_lease` refuses a lapsed lease outright. So there was **no ordering of
calls** that could certify a recovered claim. The evidence was real, the work
was on `main`, and the ledger could only answer that it fell outside the window.

Reproduced end to end on `task:36fa0badd713`, whose commit `64fb56b3` is on
`main`. The bundle was exactly right — one commit, three changed nodes, no
contamination — and conformance still answered *"evidence interval falls outside
the live claim"*. The task could only be closed by a human `resolve_task`,
which converts a mechanical check into a standing manual chore and teaches
agents that the receipt is something to be waived rather than earned.

ADR-0048 already fixed the neighbouring case: a same-owner re-claim after a
lapse *keeps* `claim_started_at` and records the hole, precisely so that work
done before the lapse stays provable. Recovery is the same situation with a
changed identity, and it did not get the same treatment.

## Decision

**Conformance judges evidence against the window that authorised the work, not
against the most recent window opened over it.**

The floor becomes the earliest window start in the audited recovery chain
leading to the current owner, and the live window when there is no such chain.
`LodestarStore::authorising_window_start` walks `claim_transfer_history`
backwards from the current owner: each hop answers "who did this identity take
the task from", so a chain of recoveries resolves to the window that started the
work.

Nothing new is stored. The transfer history already reconstructs the interrupted
window's start from the log (ADR-0064 d5), and a second copy of a fact is one
more thing that can disagree with the first.

## What this does not weaken

The guarantee in ADR-0009 is that evidence is bounded by a **real claim**, and it
survives intact:

- Every link in the chain is an audited transfer that named the owner it took
  from (ADR-0030). An identity that never held the task inherits nothing, and
  one that took the task by a fresh claim rather than a recovery inherits
  nothing either — there is no transfer record to walk.
- The floor is still the start of a window somebody actually held. Evidence from
  before *any* claim on the task is refused exactly as before, so committing
  first and claiming afterwards still cannot be certified. That is asserted by
  its own test rather than left as a consequence.
- The floor can only move earlier, never later, and only along a chain the
  ledger already recorded. Recovery remains guarded, attributed and append-only.

The behaviour is proven by a red probe: with the floor restored to the live
window the end-to-end test fails with the exact production error, and passes
with the authorising window. A test that has not been shown to fail against the
unfixed code is not evidence that the fix does anything.

## Consequences

A stranded claim can now close on its own evidence, which is what the recovery
was for. Success is measurable the way the task framed it: stranded claims
closing without a human `resolve_task`.

One edge is deliberately left alone. A task committed to *before* it was claimed
— measured at fourteen seconds on `task:36fa0badd713` — still cannot certify its
own first commit, because no claim authorised that moment. Admitting it would
mean accepting evidence from unclaimed time, which is the guarantee itself
rather than an inconvenience around it. The remedy is the existing rule that the
claim comes before the first commit, and that ordering stays load-bearing.

## Alternatives considered

**Preserve the original `claim_started_at` through recovery.** Rejected: it
makes the live window lie about when this owner took over, and `claim_window`
(ADR-0064 d6) reads that field to report lapses. One field would have to mean
two things.

**Let conformance accept any evidence for a task the agent currently owns.**
Rejected: it removes the bound rather than correcting it, and would let an agent
claim a task after the fact and certify whatever preceded it.

**Have a human `resolve_task` each stranded claim.** Rejected as the status quo:
it is the manual chore this ADR exists to remove, and every waiver teaches that
the receipt is negotiable.

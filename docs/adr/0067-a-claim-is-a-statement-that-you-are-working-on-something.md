# ADR-0067: A claim is a statement that you are working on something

- Status: Accepted
- Date: 2026-07-29
- Deciders: MindLeak maintainers
- Related: [ADR-0048](0048-a-lapsed-lease-holes-the-window-it-does-not-move-it.md)
  (lapsed leases), [ADR-0020](0020-task-lifecycle-states.md) (parking grace),
  [ADR-0024](0024-preflight-overlap-detection.md) (claims are coordination, not
  locks), [ADR-0052](0052-a-lease-is-a-heartbeat-not-a-deadline.md) (renewal on
  activity)

## Context

Measured on the live board, with six agents running:

- **36 tasks `claimed`. 4 with a live lease.** The other 32 had lapsed a median
  of 13 hours earlier; the oldest, 35 hours.
- Two agents held **15 and 14** claims apiece.
- The ledger grew by +1, +4, +10 and **+35** net tasks over four days, while 60
  pull requests merged in the last 24 hours. Delivery was not the problem.

One assumption has to be corrected before acting on any of that, because the
obvious remedy is aimed at a problem that does not exist. **A lapsed claim is
already claimable by anyone.** Both `claim_task` and `next_task` accept
`status = 'claimed' AND lease_expires_at < now`, and `stalled_work` already
reports it as `LapsedLease`. Nothing is stuck in the pool.

So "auto-release on lapse" would fix nothing and cost something real:
`release_task` nulls `claim_started_at`, which is exactly the evidence window
ADR-0048 exists to preserve across a lapse. A sweep that released stale claims
would quietly destroy the provability of work already done, to solve a
availability problem that was never there.

What is actually wrong is narrower, and it is about *reading*, not *locking*:

**The board says 36 things are in flight when 4 are.** `status` is `claimed` and
a reader — human or agent — takes that at face value. Establishing the real
number required writing a script. Every plan made against that board was made
against a number nine times too large.

**Nothing costs an agent anything for holding a claim it is not working.** There
is no limit anywhere. Fifteen claims is not a fleet working hard; it is one agent
that never let go, and the ledger presents it identically to fifteen agents each
working one thing.

## Decision

1. **An agent may hold at most `MAX_CONCURRENT_CLAIMS` (3) tasks at once.** A
   claim asserts that you are working on something, and nobody works on four
   things at once. The number is small on purpose.

2. **Lapsed claims count toward the limit.** A cap on live leases alone would
   have no teeth: letting a claim go stale costs nothing, so it would become the
   cheapest way to dodge the cap — which is the measured behaviour, not a
   hypothetical. Letting a claim go stale is not finishing it.

3. **Re-claiming a task you already hold is never a new claim.** It is the
   heartbeat path (ADR-0052) and the ADR-0048 window-preserving re-claim.
   Refusing it at the cap would turn renewing a lease into an error at precisely
   the moment an agent is doing the right thing.

4. **The refusal names what is already held and what to do about it.** A cap that
   only says "no" relocates the confusion instead of removing it: the holder
   still has to go and count, which is the work this exists to stop anyone doing.

5. **The limit is a constant, not configuration.** A limit you can raise when it
   becomes inconvenient will be raised the first time it binds — which is the
   moment it was working.

6. **`board` rows carry the derived lease state** (`live` / `lapsed`), so a claim
   nobody is holding never again reads as work in progress. Derived at read time;
   no status is rewritten and no sweep runs.

## Consequences

An agent that has accumulated claims must complete, release, or abandon one
before taking new work. For the two agents holding 14 and 15, the next claim
fails until they tidy up — which is the intended effect, and the error tells them
exactly which tasks and what to do.

This does **not** rewrite status on lapse, add a background sweep, or change
`release_task`. The established idiom here is to widen the claim query rather
than to mutate rows on a timer (ADR-0020's parking grace works the same way), and
ADR-0063 is a recent, expensive reminder of what happens when a migration edits
live claim state underneath its holder.

It also does not, on its own, make the backlog shrink. The cap addresses claims
that are not being worked; it says nothing about *why* tasks are created faster
than they are closed. That is a separate question about what deserves to be a
task at all, and it is not answered here.

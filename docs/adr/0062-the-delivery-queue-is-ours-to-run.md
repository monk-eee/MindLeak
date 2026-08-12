# ADR-0062: The delivery queue is ours to run

- Status: Accepted
- Date: 2026-07-28
- Related: [ADR-0061](0061-delivery-is-queued-not-raced.md) (delivery is queued,
  not raced),
  [ADR-0045](0045-a-fleet-is-a-distributed-system.md) (armed means finished),
  [ADR-0038](0038-isolated-worktrees-shared-repository-state.md) (isolated
  worktrees, shared repository state),
  [ADR-0049](0049-publication-requires-a-claim.md)
  (the ledger is not optional at the publication boundary)

## Context

ADR-0061 decided that delivery is queued rather than raced, and chose GitHub's
merge queue as the mechanism. The reasoning holds. The mechanism is not
available to us.

Attempting to enable it returns:

```
POST /repos/monk-eee/MindLeak/rulesets
422  Validation Failed
errors: ["Invalid rule 'merge_queue': "]
```

This is not a permissions problem or a malformed payload. The same endpoint,
with the same credentials, accepts a ruleset containing a `non_fast_forward`
rule and then deletes it cleanly; only the `merge_queue` rule type is refused.
`monk-eee/MindLeak` is a public repository owned by a **user account**, and the
merge queue requires an organisation-owned repository. Moving the repository to
an organisation is not available to us either.

So ADR-0061 is accepted, correct, and unimplementable as written — and its CI
half has already landed, leaving a `merge_group` trigger in
[`ci.yml`](../../.github/workflows/ci.yml) that nothing will ever fire.

Meanwhile the problem it named is live and measurable. With
`required_status_checks.strict` enabled and eleven armed pull requests, every
merge makes the other ten stale. Each stale branch that updates itself burns a
full five-check run against a `main` that the next merge invalidates again.
That is O(N²) check runs for N queued changes, and at N=11 the queue does not
drain: branches spend longer going stale than they do going green.

The tempting shortcut is to take ADR-0061's second half alone and set `strict`
to `false`. ADR-0061 rules this out in advance, and it is right to: dropping
up-to-dateness without a queue is what allows two individually-green branches to
merge into a broken `main`. Four of the eleven open changes are refactors
splitting the *same* modules. That is exactly the population where two green
branches combine badly.

## Decision

**Run the queue ourselves, as a serialiser for branch updates only.**

1. **The queue does not merge.** Merging stays with GitHub's auto-merge, gated
   by the same five required checks and the same branch protection as before. An
   agent holding merge rights would be a second path into `main` that protection
   does not govern — a worse problem than the one being solved, and a direct
   contradiction of ADR-0049's insistence that the publication boundary is not
   optional. The queue decides only *whose turn it is to update*, which is the
   thing nobody currently decides.

2. **Armed means queued.** A pull request with auto-merge enabled is one whose
   author has declared it finished (ADR-0045). That is the membership test, so
   there is no new label, list, or ritual to remember. Ordering is
   first-in-first-out by *when it was armed*, not when it was opened, so a
   long-lived branch finished last does not jump work that was ready first.

3. **Exactly one branch updates at a time.** This is the whole mechanism. A
   second update while the first is still building invalidates the first before
   it can land, which is the race itself, reintroduced by the thing meant to
   remove it.

4. **Nothing may wedge the queue permanently.** A check that never reports stops
   being waited on after a stall threshold. A branch with a real conflict is
   reported and stepped over — it needs reconciling in its own worktree
   (ADR-0038), not a place at the head of the queue. A branch with failing
   checks is stepped over too: updating it would only burn CI to fail again.

5. **`strict` stays on.** With the queue serialising updates, up-to-dateness is
   cheap to maintain — each branch is brought current once, when its turn
   arrives, against the `main` that actually resulted from the merge before it.
   O(N) runs instead of O(N²). The safety property ADR-0061 warned about keeping
   is kept.

## Consequences

Delivery drains again without weakening any gate. The five required checks, the
protected branch, and the publication ledger are all untouched; the only thing
that changed is that update turns are taken in order instead of simultaneously.

It is a local agent, not infrastructure. Nobody has to run it for the repository
to be correct — an unattended queue just means branches go stale the way they do
today. That is a deliberate property: a delivery mechanism that becomes
load-bearing is one that can fail closed, and this one degrades to the status
quo rather than to a stoppage.

We accept a weaker guarantee than a real merge queue gives. GitHub's queue tests
the *prospective merged result* of several pull requests together; this tests
each branch against the `main` it will actually merge into, one at a time. That
is exactly the guarantee `strict` already provides — no stronger, and no weaker.
Batching is what we give up, and at this fleet's volume batching is a throughput
optimisation rather than a safety property.

ADR-0061 is not superseded. Its decision stands and should be implemented the
moment the mechanism becomes available; this records why it could not be, and
what we do until then. The orphaned `merge_group` trigger in CI is left in place
deliberately — it is harmless, and removing it would only have to be undone.

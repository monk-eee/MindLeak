# ADR-0058: Work that shipped must be able to leave the board

- Status: Proposed
- Date: 2026-07-28
- Related: [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed
  conformance), [ADR-0019](0019-task-retention-and-board-hygiene.md) (task
  retention and board hygiene),
  [ADR-0048](0048-a-lapsed-lease-holes-the-window-it-does-not-move-it.md) (a
  lapsed lease holes the window),
  [ADR-0053](0053-the-graph-records-events-not-conclusions.md) (the graph
  records events, not conclusions),
  [ADR-0057](0057-work-already-done-is-a-collision.md) (work already done is a
  collision)

## Context

The work board carried 158 tasks: 75 done, **61 abandoned**, 16 claimed, and
five genuinely live items. Of the sixteen claimed, **thirteen had lapsed leases
and every one described work that had already merged.** The design board showed
the same shape: 55 designs, of which six sat `accepted` with promotion still
`pending`, and five of those six were implemented and on `main`.

The obvious reading is that agents forget to close things. That reading is
wrong, and the evidence is specific.

Following one task to completion with its real merge commit produced
`needs_human — evidence contains no provenance-bearing mutation`. So did the
next. So did the third. The cause was three layers down:

- `evidence_for` counts an intent as a commit only when it carries a
  `refactored` edge;
- `ingest_commit` writes those edges from an argument named `changed_files`;
- the caller passed `files`, and the Memory Plane **dropped it in silence**.

No edges, therefore no commit, therefore no provenance-bearing mutation,
therefore `needs_human`, therefore the task cannot reach `done`. **The board was
not full of forgetfulness. It was full of work that the ledger had no way to
accept.**

That specific defect is fixed — the Memory Plane now refuses an argument it does
not declare. The structural problem it exposed is not, and it is worth naming
before it recurs in another shape: **`complete_task` is the only route out of a
claim, it requires evidence the agent must assemble by hand, and every failure to
assemble it correctly is indistinguishable from work that was never done.**

Meanwhile the actual proof was sitting in plain view: a merged pull request with
green checks, referenced by the branch the task was claimed on.

## What testing the fix changed about this decision

The first draft of this ADR proposed the mechanism below as necessary. Testing
the argument fix first showed it is not, and the record should say so rather
than quietly ship a feature whose justification had evaporated.

With the argument named correctly, a commit ingested inside the claim window
produces exactly what conformance wanted:

```
ingest_commit -> { "edges_created": 1, "nodes_created": 1 }
evidence: commits=1; changed=1
  commit_ids: ["intent:210a06ed…"]
  changed:    ["artifact:crates/mindleak-storage/src/repository.rs"]
verdict: needs_human — evidence does not touch code bound to the task goal
```

The "no provenance-bearing mutation" failure is gone, and the verdict that
replaces it is a *correct* one about that probe task's goal binding. **The
silent argument drop was the whole disease.** Tasks can close today.

One real limitation survives, and it is narrower than the first draft claimed:
re-ingesting an already-known commit upserts it without moving its timestamp, so
a commit made *before* the claim cannot be pulled into the window afterwards.
That is correct behaviour — it is the ADR-0048 property that stops an agent
back-dating evidence — but it means work whose commits were never ingested at
the time cannot be retro-proved. Thirteen claims are in that position now.

So this decision is **not** proposed as a fix for closure, which works. It is
proposed, if at all, as a way to prove work whose commits were never ingested —
a smaller claim, and one that may not be worth a mechanism.

## Decision

**A merge is evidence, and the ledger should be able to consume it.**

1. **`complete_task` accepts a merge reference** — a commit on `main` whose
   branch was the claimed branch (ADR-0057 records that branch at claim time).
   The plane verifies it: the commit is reachable from `main`, and it touches
   paths within the task's declared scope. Deterministic, no model, no trust in
   the caller's summary.

2. **A verified merge is a provenance-bearing mutation.** It is stronger than
   the hand-assembled bundle it replaces: a merge to a protected branch has
   passed review and CI, which is more than an ingest call can attest.

3. **Conformance still judges.** This changes what evidence *is*, not whether it
   is examined. A merge that touches nodes governed by a goal the task does not
   serve is still drift, and still lands in review.

4. **The board reports what it cannot close.** A claim whose lease lapsed while
   its branch merged is a specific, nameable state, and it should appear as one
   rather than as an indefinite claim. Reporting is enough; ADR-0019 already
   settled that terminal work is archived, not deleted.

5. **Nothing closes a task automatically.** The agent, or a human, still calls
   `complete_task`. This removes the requirement to *manufacture* evidence, not
   the requirement to *submit* it — an auto-closing board would record
   completions nobody attested, which is the failure ADR-0009 exists to prevent.

## Consequences

Tasks become closable by the artefact the fleet already produces, which is the
only reason to expect the board to stay clean without ceremony nobody performs.

The 61 abandoned tasks are not addressed here. They predate the publication gate
and are documented in `claim-gate.mjs`: *"one night of nine merged pull requests
produced 61 abandoned tasks, two claim owners across twenty-three agent
identities, and no receipts at all."* They are history, and archiving them from
the default view is a separate, smaller decision.

The risk is that a merge reference becomes a rubber stamp — an agent points at
any merged commit and collects a receipt. Decision 1's verification is what
stops that, and it must stay deterministic: the moment "does this commit
correspond to this task" becomes a judgement call, this becomes a way to launder
work rather than to prove it.

## Alternatives considered

**Make the evidence bundle easier to assemble.** The immediate cause was a
dropped argument, now refused. But the deeper problem stands: the agent must
still construct a bundle describing work it has already finished, at the moment
it cares least, and any error is silent and terminal. Better ergonomics on a
path nobody wants to walk is not a fix.

**Close tasks automatically when their branch merges.** Rejected under ADR-0009.
Nobody attested; the receipt would assert a completion no agent claimed, which is
exactly the "aligned receipt with nothing behind it" that evidence-backed
conformance exists to prevent.

**Let the human sweep the board periodically.** This is the status quo, and the
board reached 39% abandoned and 13 zombie claims in a single day. A hygiene task
that competes with real work loses.

**Abandon the stale claims.** Considered and refused during this investigation:
`abandon_task` accepts expired claims, but recording shipped work as abandoned
puts a falsehood in the ledger. A lying ledger is worse than an empty one,
because it reads as governed.

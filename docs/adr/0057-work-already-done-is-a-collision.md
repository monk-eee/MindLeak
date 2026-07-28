# ADR-0057: Work already done is a collision the fleet cannot see

- Status: Proposed
- Date: 2026-07-28
- Related: [ADR-0015](0015-advisory-symbol-leases.md) (false safety is worse
  than none), [ADR-0038](0038-isolated-worktrees-shared-repository-state.md)
  (isolated worktrees, shared repository state),
  [ADR-0049](0049-publication-requires-a-claim.md) (publication requires a
  claim), [ADR-0056](0056-the-changelog-is-assembled-not-edited.md) (the
  changelog is assembled, not edited)

## Context

One day, one repository, roughly ten concurrent agents across 42 worktrees:
60 pull requests, 54 merged, 311 commits on `main`.

**Nine of those sixty existed only to redo another one.**

```
#72 supersedes #70   #65 supersedes #64   #63 supersedes #61
#62 supersedes #60   #59 supersedes #56   #57 supersedes #55
#42 supersedes #38   #53 supersedes an earlier attempt
#71 restores a Memory Plane merge that a supersede had lost
```

That last line is the sharp one: rework that generated further rework. Fifteen
per cent of the day's output was a second attempt at work that already existed,
and **the coordination plane prevented none of it.**

It is not that coordination failed at what it does. Nothing was clobbered, no
agent lost work to another, and every claim held. Claims are scoped to *paths*,
and almost none of these collisions were path collisions.

They were **staleness** collisions, and they share one shape:

1. An agent finishes work and arms auto-merge — "armed means finished".
2. Something unrelated lands on `main`.
3. The pull request goes `DIRTY` or `BEHIND` and stops being mergeable.
4. Someone — often a *different* agent — sees a stalled pull request and opens a
   fresh one against current `main`.

At step 4 the ledger already knew everything needed to prevent it. The original
branch had a claimed task; that task served a goal or materialised a design; the
session had declared its branch through `open_session` (ADR-0035, ADR-0044).
Every fact was present. **Nothing joined them**, so the second agent was told
nothing, and the honest thing to say about a system that had the data and stayed
silent is that this is a gap, not bad luck.

`check_overlap` is the nearest thing and it does not help here. It answers "who
else holds a claim over these paths *right now*", so it misses the case where
the first agent's work is **complete and unmerged** — which is exactly when
redoing it is most tempting and most wasteful. Worse, its false-positive rate is
what teaches agents to skip it: it fired on my own claim once, and an advisory
signal that names the wrong party trains readers to discount the next one.

## Decision

**Make "this work already exists" a first-class answer, derived from facts the
ledger already holds.**

1. **A task records the branch it is being done on.** `claim_task` already
   accepts a session, and `open_session` already declares that session's branch.
   Join them at claim time and store it. No new declaration is asked of anyone.

2. **A new read: `existing_work(design_id | goal_id | paths)`** returns tasks and
   their branches that already serve the same design or goal, *including
   completed ones whose branch is unmerged*. This is the query that was missing:
   not "who is touching this file" but "has this already been done".

3. **`create_task` reports a prior task serving the same design.** It does not
   refuse — a second task against one design is often legitimate, and a gate
   here would be wrong more often than right (ADR-0015: false safety is worse
   than none). It states the fact and names the branch, at the one moment the
   second agent is deciding whether to start.

4. **`canonical-push` names a sibling branch serving the same design**, in the
   same advisory register as the overlap notice. Publishing anyway stays fine.
   Building the same thing twice is what we are trying to stop.

5. **Superseding is recorded, not inferred.** When a pull request genuinely does
   replace another, that is a human act with a reason, exactly as
   [ADR-0050](0050-a-superseded-decision-is-not-a-stale-one.md) established for
   designs. The system must not guess that two branches serving one design are
   duplicates — they may be a deliberate split.

## Consequences

The measurable outcome is the rework rate. Today it was 9/60; if this works it
should fall, and if it does not fall the mechanism is wrong and should be
removed rather than tuned indefinitely.

Advisory notices accumulate, and each one that misfires devalues the rest. This
adds a third (`existing_work`) beside claim overlap and identity collision. That
is the main risk, and the reason for decision 3's "report, never refuse": a
notice that is wrong is survivable, a gate that is wrong stops the fleet.

Storing the branch on a task adds a field that can go stale — an agent may
switch branches mid-task. Stale is acceptable here because the notice names the
branch it believes, so a human reading it can see immediately that it is wrong.
A wrong *name* is self-correcting in a way a wrong *verdict* is not.

## Alternatives considered

**Make auto-merge resilient instead.** Most of the nine were caused by armed
work going stale, and ADR-0056 already removed the largest single cause by
taking `CHANGELOG.md` out of every branch. This is worth doing and is not
sufficient: two agents can still start the same work from a clean slate, and
merge-queue hygiene cannot see intent.

**Refuse a second task against a design that already has one.** Rejected under
ADR-0015. Splitting one design across two tasks is normal, and a refusal would
be wrong often enough that agents would route around it — which is how a gate
becomes worse than nothing.

**Infer duplication from the graph.** MindLeak observed all nine collisions.
Inferring "these two branches are the same work" needs a judgement about
intent, and either an LLM on the coordination path — which invariant 1 forbids —
or a similarity heuristic that would be confidently wrong. The Intent Plane
holds the *declared* answer; use that instead of guessing at it.

**Do nothing and accept the rate.** Defensible if the rate is low. It is 15%,
measured, on the day the fleet was most productive.

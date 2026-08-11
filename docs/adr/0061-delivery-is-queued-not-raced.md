# ADR-0061: Delivery is queued, not raced

- Status: Accepted (remedy blocked)
- Date: 2026-07-28
- Related: [ADR-0038](0038-isolated-worktrees-shared-repository-state.md)
  (isolated worktrees, shared repository state),
  [ADR-0045](0045-a-fleet-is-a-distributed-system.md) (armed means finished),
  [ADR-0049](0049-publication-requires-a-claim.md) (publication requires a
  claim), [ADR-0056](0056-the-changelog-is-assembled-not-edited.md) (the
  changelog is assembled, not edited)

## Context

`main` requires every pull request to be up to date before it may merge:

```
required_status_checks.strict   : true
required checks                 : 5
required_approving_review_count : 0
```

With one contributor that setting costs nothing. With a fleet it is a
serialisation point that grows quadratically, because **every merge invalidates
every other open pull request**:

1. A pull request goes green on all five checks.
2. Some other pull request merges.
3. It is now `BEHIND`, and its green result is void.
4. `update-branch` restarts all five checks from zero.
5. Return to 1.

Measured over twenty-four hours on this repository:

| | |
|---|---|
| CI runs | 200 |
| distinct branches | 71 |
| repeat runs | 129 |
| **share of CI spent re-running unchanged code** | **65%** |

The merge cadence over the preceding three hours was `+10, +8, +5, +9, +6`
minutes. That is not congestion — it is exactly one CI duration per merge.
Throughput is **one pull request per CI cycle regardless of how many are
armed**, so a queue of eleven takes one to two hours to drain, and a fleet that
opens work faster than that never drains at all.

The failure is quiet, which is what makes it expensive. Nothing reports it: each
pull request looks healthy, auto-merge stays armed, checks pass. A board of
eleven green-then-stale pull requests reads as "busy" rather than "livelocked",
and the natural response — arm more, update more often — makes it worse.

This is the third time the same shape has been recorded here. ADR numbers, the
ADR index and `CHANGELOG.md` were each a shared resource every branch had to
touch, and each was fixed by removing the contention rather than by asking
agents to contend more politely. `strict` is the same shape at the level of the
branch itself: the shared resource is `main`, and every branch is required to
have just observed it.

## Decision

**Enable GitHub's merge queue on `main` and stop requiring each branch to be
individually up to date.**

- Merge queue enabled on `main`.
- The five required checks are unchanged and still gate entry.
- `required_status_checks.strict` becomes `false`.

The queue builds the prospective merged result, tests *that*, and merges in
order. Up-to-dateness stops being a property each branch must maintain against a
moving target and becomes a property the queue establishes once, at the point
where it actually matters. Testing what will land is strictly stronger than
testing each branch against a `main` that has already moved on.

Dropping `strict` without the queue would be a regression, not a simplification:
it is what stops two individually-green branches merging into a broken `main`.
The two halves are one change.

### The order is load-bearing

A merge queue runs the required checks against a temporary `merge_group` ref
holding the prospective merged result. **A required check that does not trigger
on `merge_group` never reports**, so the queue waits for it forever and nothing
merges at all — a strictly worse outcome than the problem being solved, and one
that presents as the queue being "slow" rather than as a misconfiguration.

All five required checks come from `ci.yml`, which triggered only on `push` to
`main` and on `pull_request`. So:

1. `merge_group:` is added to `ci.yml` **first**, and merged. It is inert until
   a queue exists, so it is safe to land on its own.
2. Only then is the queue enabled and `strict` dropped.

This ADR ships step 1. Step 2 is a settings change on `main`.

### It cannot be scripted

Classic branch protection does not expose the merge queue through either REST or
GraphQL: `UpdateBranchProtectionRuleInput` has no `requiresMergeQueue`, and
`BranchProtectionRule` has no such field to read back. The setting is reachable
only through the repository settings UI, or by migrating `main` from classic
protection to a ruleset with a `merge_queue` rule.

That is worth recording rather than discovering twice. It also means the change
cannot be captured in this repository the way the rest of its policy is, which
is the strongest argument for the ADR: the reasoning has nowhere else to live.

### The remedy is unavailable here, and finding out cost protection

The absent API turned out not to be an API gap. **GitHub's merge queue requires
an organization-owned repository.** This one is owned by a user account:

```
owner type : User
visibility : public
```

So there is no "Require merge queue" checkbox on the branch protection page at
all — absent, not disabled — and no field behind REST or GraphQL to set. Nothing
was misconfigured and nothing was missed.

Attempting it did real harm for about twenty minutes. `strict` was unticked and
the queue could not be ticked, leaving exactly the half-state this ADR names as
a regression: two individually-green branches able to merge into a broken
`main`. It was restored by `PATCH .../required_status_checks {"strict": true}`,
required checks unchanged.

That is the part worth carrying forward. The ADR already said the two halves are
one change; the failure mode when they separate is not theoretical, and the
half that is *easy* to apply is the half that removes protection. **Verify the
queue exists before dropping `strict`, not after.**

### What remains available

The measurement stands — 65% of CI in twenty-four hours spent re-running
unchanged code, and throughput of one pull request per CI cycle regardless of
how many are armed. Only the remedy is out of reach.

**Decided: accept the churn (monk-eee, 2026-07-28).**

The cost is real and is being paid knowingly. It scales with the number of
simultaneously-armed pull requests rather than with the size of the repository,
so it is tolerable at the current fleet size and becomes the trigger to revisit
when it is not.

**Moving the repository to an organisation remains the exit** and unlocks the
queue exactly as described above. It is a decision about where the project
lives, not a CI tweak, so it is not taken here.

A third option — reducing contention by arming fewer branches at once — was
considered and rejected. It asks the fleet to be smaller to suit the merge
policy, which inverts the goal (ADR-0038), and it cannot be enforced, so it
degrades silently under exactly the load that makes it matter. That is the same
objection this ADR already raises against "arm fewer pull requests at once" in
the alternatives below; listing it as an available option contradicted the
document's own reasoning.

`merge_group` in `ci.yml` stays. It is inert without a queue, costs nothing, and
means the organisation route is a single settings change on the day it is taken
rather than a prerequisite to rediscover.

## Consequences

**Auto-merge keeps its meaning.** ADR-0045 says armed means finished. Today that
is only true in the absence of contention — armed work goes stale and stops. The
queue is what makes the claim honest, so this ADR is a repair of ADR-0045 rather
than a new policy.

**A failure in the queue names a batch, not a branch.** When a batch fails, the
queue bisects and ejects the offender, and the diagnosis is briefly less obvious
than "this pull request is red". That is the real cost, and it is smaller than
65%.

**`update-branch` stops being routine.** It stays available for genuine
divergence, but the reflex of updating a stale-but-green branch disappears —
along with the CI it was spending.

**This is a settings change, not a code change.** Nothing in the repository
enforces it and no test can, which is precisely why it belongs in an ADR: the
reasoning would otherwise live only in a settings page nobody reads and anybody
can quietly reverse.

## Alternatives considered

**Leave it and arm fewer pull requests at once.** Asks the fleet to be smaller
to suit the merge policy, which inverts the goal — the fleet is the point
(ADR-0038). It also cannot be enforced, so it degrades to a convention that
fails silently under load.

**Drop `strict` alone, no queue.** Removes the churn and removes the protection
with it: two branches that are each green against an older `main` can merge into
a broken one. The setting exists for a real reason.

**Reduce the required checks so re-runs are cheaper.** Treats the symptom by
lowering the bar. The checks are the evidence this repository's whole
conformance model rests on; making them thinner to survive a queueing problem
trades the thing being protected for the cost of protecting it.

**Batch by hand: disarm everything, merge one, re-arm.** What is happening now,
manually and by whoever notices. It works and it is unpaid labour that recurs
every day, which is the definition of something to automate.

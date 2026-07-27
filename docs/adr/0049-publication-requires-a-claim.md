# ADR-0049: Publication requires a claim; the ledger is not optional

- Status: Accepted
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Refines: [ADR-0045](0045-a-fleet-is-a-distributed-system.md) (a fleet is a
  distributed system), [ADR-0009](0009-evidence-backed-conformance.md)
  (evidence-backed conformance)
- Related: [ADR-0024](0024-preflight-overlap-detection.md) (pre-flight overlap),
  [ADR-0030](0030-discrete-per-agent-identity.md) (per-agent identity),
  [ADR-0038](0038-isolated-worktrees-shared-repository-state.md) (isolated
  worktrees, reviewed convergence)

## Context

The Intent Plane had exactly one real arbiter — `claim_task`, a genuine
compare-and-swap — and **zero automatic integration points**. Nothing in
`pre-commit`, `scoped-commit`, `canonical-push`, or CI ever consulted it.
`advise` is documented as never gating. `check_overlap` reports. `next_task`
suggests. Every one of them had to be called on purpose.

That is not a plane being *bypassed*. It is a plane that is **optional by
construction**, and one night of concurrent work measured what optional costs:

- 9 pull requests merged, **0 conformance receipts**
- 61 done tasks and **61 abandoned** ones — work planned in the ledger, done
  outside it, and the entry discarded rather than completed
- **2 claim owners** across **23 agent identities**
- 7 tasks stalled in `in_review` / `blocked` / `needs_input`, three of them
  describing work that was then done anyway on branches
- two agents independently building overlapping answers to the same question
  (`stalled_work` and the fleet wait graph), discovered only when both pull
  requests were open

None of that is a discipline failure. Voluntary consultation under time pressure
is consultation that does not happen, and a governance plane nobody consults is
not weak governance — it is decoration that reads as governance, which is worse
than none because it invites trust.

The seam needed to fix it already existed and was empty: the `pre-push` hook
refuses any push that did not come through `canonical-push`. Every commit that
becomes visible to another agent already passes through one script.

## Decision

**Publication requires a live claim. Commits do not.**

1. **The gate is at push, not commit.** A commit is a draft — cheap, frequent,
   exploratory. Gating commits makes people invent tasks to satisfy the check,
   filling the board with plans written after the work: a *lying* ledger, which
   is worse than an empty one because it reads as governed. A push is where work
   becomes visible to the fleet, and one branch is an honest unit of work.
2. **`canonical-push` refuses without a live claim owned by this agent**, naming
   `claim_task` and `create_task` as the actions that satisfy it. A lapsed lease
   is not a claim: re-claiming opens the fresh evidence window that is the whole
   point of the record.
3. **Identity is required first.** No `LODESTAR_AGENT`, no publication
   (ADR-0030). An unattributed push is a receipt for nobody.
4. **An unreachable ledger refuses.** This deliberately contradicts the
   auto-merge guard, which permits when `gh` cannot answer. The distinction is
   real: `gh` being absent is an ordinary condition, while Lodestar is local
   SQLite behind a local binary, so unreachable means broken. A gate that fails
   open turns "the ledger was down" into the universal bypass within a week, and
   a gate with a universal bypass is decoration again.
5. **Overlap is reported, never enforced.** Two branches legitimately touch one
   file, so a gate would be wrong far more often than right (ADR-0024). The
   value is that a human sees the collision at the one moment it is still
   cheap — before both agents have built the same thing.

## Consequences

- The ledger stops being optional at exactly one boundary, and that boundary is
  already mechanically enforced. Nothing new has to be obeyed.
- Publishing now costs a claim. That is the intended friction: it is the price
  of the branch existing in the record at all, paid once per branch rather than
  once per commit.
- Lodestar becomes a hard dependency of publishing. A broken ledger stops the
  fleet publishing, which is severe and is the point — the alternative is a
  gate that quietly does nothing.
- Local iteration is untouched. Commit, rebuild, run tests, commit again; the
  ledger is consulted when the work leaves the worktree.
- The overlap notice will sometimes name a collision that is fine. Accepted:
  it costs a line of output, and the failure it catches cost two agents a
  duplicated feature in one night.

## Rejected alternatives

- **Gate the commit.** Stricter on paper. It converts a missing-record problem
  into a fabricated-record problem, and only the second is hard to detect. It
  also cannot catch the overlap and merge failures, which happen at branch
  boundaries and not at commits.
- **Keep it advisory and rely on discipline.** Already the design, already
  measured: 61 abandoned tasks and zero receipts in a single night.
- **Fail open when the ledger is unreachable.** Consistent with the auto-merge
  guard and wrong here, for the reason in decision 4. Consistency between two
  guards is not a goal; being right about each one is.
- **Require the claim's declared scope to cover the branch's changed paths.**
  Attractive and premature. Scope is advisory and frequently unset, so this
  would refuse honest work for a missing optional field. Report the mismatch
  first; tighten only if the data says the report is being ignored.
- **Have `canonical-push` open a task automatically when none is claimed.** Zero
  friction, and it produces a ledger of retrospective auto-tasks that describe
  what happened rather than what was intended. That is a receipt log wearing an
  intent plane's clothes, and it would make the board's numbers look healthy
  while meaning less than they do now.

# ADR-0147: A claim reports the branch's existing governed files, not only its own declared scope

- Status: Accepted
- Date: 2026-08-30
- Deciders: MindLeak maintainers
- Accepted: 2026-08-30 by the repository owner, authorized directly in
  session — attributed human adoption after review.
- Related: [ADR-0009](0009-evidence-backed-conformance.md),
  [ADR-0015](0015-advisory-symbol-leases.md) (the advisory-not-a-refusal
  principle it establishes, reused by `live_task_titled`'s own doc comment),
  [ADR-0041](0041-cross-cutting-work-is-declared.md)

## Context

`gaps.d/consecutive-tasks-on-one-branch-inherit-governed-scope.md` measured a
real, repeated coordination failure, twice, on two different branches:

> `task:6d751963490d` changed 4 files and its evidence listed 10. The extra six
> were `task:c67ccd90a9a3`'s work, already committed and pushed on the same
> branch. One of them, `scripts/scoped-commit.mjs`, is governed by
> `goal:an-agent-commits-only-in-a-working-tree-it-owns`... the earlier claim
> had declared [it] in `also_serves` and the later one had no reason to. Both
> tasks were correct; both verdicts were correct.

A second, independent measurement on a different branch (recorded as durable
`learned` knowledge, `conformance_id: 1346`) corrected the fragment's own
original diagnosis:

> The real cause is simpler and worse: a task's evidence is every commit on
> the branch since its base, so ANY unmerged prior work on that branch is
> inherited — no merge required.

This is not a bug in git plumbing; it is exactly what `scripts/canonical-
push.mjs` already computes on purpose (line 203,
`git diff --name-only ${remote}/main...HEAD`) — the branch's entire diff
since its base, because that is genuinely what "this push makes visible to
the fleet." The problem is not that evidence is wide; the problem is that
the first time an agent learns it is wide is at completion, as a `drift`
finding naming a file it did not touch this claim. The same measurement
confirmed the available exit already works once found:

> declaring the inherited goals in `also_serves` AT CLAIM TIME converts the
> evidence-inheritance verdict from `drift` to `needs_human`... the
> mechanism working exactly as designed... before claiming task N+1, look at
> what is already on the branch since its base and declare the goals
> governing it.

The fragment names two candidate remedies and explicitly declines to choose:
"choosing between 'warn at claim time' and 'widen the window's definition' is
a design decision, not a patch." It also names the mechanism the first remedy
would reuse: "`task_claim` already computes governing clauses for the
declared paths; extending that to the branch's existing diff is the same
query against a wider set" — referring to `Lodestar::governing_clauses_for_task`
(`facade/advice.rs`), which today resolves governing clauses only from
`store.artifacts_for_goal(&task.goal_id)` and
`store.governed_nodes_for_task_scope(task_id)` — the task's own goal and its
own declared `paths`/`symbols`, never anything already sitting on the branch
it was claimed on.

Per this skill's own operating rule, Lodestar never inspects Git itself —
`branch`, `head_sha`, `base`, and `dirty` are caller declarations it records,
not facts it verifies. Any fix that needs "what does this branch already
contain" has to receive that as a declared input, exactly like every other
git fact Lodestar already treats this way.

## Decision

**`task_claim` accepts an optional, caller-declared list of paths already
committed on the branch since its base, and reports the governing clauses for
those paths separately from the clauses governing the task's own declared
scope. Evidence semantics do not change.**

1. **`task_claim(step="claim")` gains an optional `branch_committed_paths:
   Vec<String>`.** The caller computes it exactly the way `canonical-
   push.mjs` already computes its own `changed` variable —
   `git diff --name-only <base>...HEAD` — but runs it *before* claiming
   instead of only at publish time. This is a declaration, not a fact
   Lodestar verifies, matching `branch`/`head_sha`/`base` in `open_session`;
   an honest caller supplies it, and nothing here can force one that omits it.
2. **`governing_clauses_for_task` computes two sets, not one, when
   `branch_committed_paths` is supplied.** The existing `governing` field
   keeps meaning exactly what it means today: clauses governing the task's
   own goal and its own declared `paths`/`symbols`. A new `branch_inherited`
   field reports clauses governing `branch_committed_paths` that are *not*
   already covered by the task's own declared scope or its existing
   `also_serves` coverage. The two are never merged into one list — an agent
   must be able to tell "this governs what I am about to change" from "this
   already governs something already sitting on my branch" at a glance.
3. **This is advisory, never a gate (ADR-0015).** The claim succeeds exactly
   as it does today whether or not `branch_committed_paths` is supplied, and
   regardless of what `branch_inherited` reports. Nothing here refuses a
   claim, widens what a later `also_serves` declaration is timed against, or
   changes when `declare_coverage` is refused (ADR-0041: only until the first
   conformance record exists for the task).
4. **Absent input degrades to exactly today's behaviour.** A caller that does
   not supply `branch_committed_paths` — older tooling, a fresh branch with an
   empty diff, or a caller that genuinely has none to report — sees the same
   `governing` field it sees today and no `branch_inherited` field at all.
   This is purely additive; nothing regresses for a caller that ignores it.
5. **Evidence semantics are explicitly out of scope and unchanged.** This ADR
   rejects "widen the window's definition" as the fix. `changed_node_ids` and
   the conformance verdict it feeds continue to mean exactly what ADR-0009
   already guarantees: everything on the branch since its base, full stop,
   regardless of which task's claim window an agent believes a given commit
   mentally belongs to. Narrowing that to "only the commits I meant this
   task to cover" would reopen the exact vulnerability ADR-0009 exists to
   close — a claim retroactively deciding which of its own commits count,
   which is indistinguishable from laundering a finding after the fact,
   and is exactly as dangerous whether the narrowing is deliberate or an
   honest agent's own imperfect memory of which commit belonged to which
   task (a real failure mode this repository has already measured: a
   compaction reliably supplies a second, unfamiliar context for the same
   branch, per `gaps.d/rescuing-a-lapsed-lease-can-duplicate-a-published-
   pr.md`). An evidence bundle that is reconstructable purely from git state
   — with no dependency on an agent correctly remembering its own past
   intent — is worth more than one that reads cleaner per task.
6. **The natural caller for `branch_committed_paths` is whatever already
   declares `branch`/`base` at claim time.** For a session driving Lodestar
   directly, that is the same call site that already runs `open_session`
   with a non-empty `branch` matching an existing remote ref; for the
   documented recovery path (`scripts/mcp-direct.mjs`), it is one additional
   `git diff --name-only` call before building the claim payload. Neither is
   part of this ADR's scope to wire — see Consequences.

## Consequences

- An implementing agent's natural slice order: (1) the new
  `branch_committed_paths` parameter and `branch_inherited` response field in
  `lodestar-core` (`facade/advice.rs`, `facade/executive/claim.rs`), proven by
  a test that claims a second task on a branch already carrying a governed
  file the first task declared and asserts it appears in `branch_inherited`
  but not `governing`; (2) the MCP surface argument in whichever tool module
  exposes `task_claim`; (3) wiring the caller side — `scripts/claim-gate.mjs`
  and/or `scripts/mcp-direct.mjs` computing the diff before calling claim —
  which is real, separate follow-up work, not part of authoring this ADR.
- `gaps.d/consecutive-tasks-on-one-branch-inherit-governed-scope.md` should be
  updated once the above ships, to record the remedy rather than only the
  still-open question; not part of this ADR, which is design-only.
- The procedural remedy the fragment already names — one branch per task
  (ADR-0038) — remains the correct default and is unchanged by this ADR. This
  decision exists for the case ADR-0038 does not eliminate: a branch that
  legitimately outlives one task, which the delivery queue's own operation
  makes a natural, recurring shape rather than a deviation to eliminate.

## Rejected alternatives

**Redefine `changed_node_ids` to exclude files a prior task on the same
branch already covered.** Rejected per decision 5: this requires trusting an
agent's own attribution of which commit belongs to which task, which is
exactly the property ADR-0009 exists to not depend on, and which this
repository has already measured failing across a context compaction.

**Gate the claim on declaring `branch_committed_paths` (refuse an
undeclared branch reuse).** Rejected: this repeats the reasoning ADR-0048's
own design note already established for lease gating — a hard gate teaches
an agent to route around it (an empty or fabricated declaration satisfies a
gate without being honest), where a warning that costs nothing to ignore
and nothing to heed stays honest either way.

**Have Lodestar compute the branch diff itself.** Rejected: Lodestar
deliberately never inspects Git (this skill's own Session Invariant); adding
one call site that does would be a narrower, inconsistent exception rather
than a considered reversal of that boundary.

**Do nothing beyond documenting the workaround in the gap fragment.**
Rejected: the workaround already works today, but only for an agent who
thinks to run it — the fragment itself frames the residual as "a design
decision, not a patch," and leaving it undecided is why it stayed open across
two independent measurements on two different branches.

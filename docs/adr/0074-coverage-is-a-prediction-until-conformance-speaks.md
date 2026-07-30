# ADR-0074: Coverage is a prediction until conformance speaks

- Status: Accepted
- Date: 2026-07-30
- Decider: MindLeak maintainer
- Related: [ADR-0041](0041-cross-cutting-work-is-declared.md) (cross-cutting work
  is declared), [ADR-0029](0029-proactive-constitutional-advice.md) (proactive
  constitutional advice),
  [ADR-0048](0048-a-lapsed-lease-holes-the-window-it-does-not-move-it.md) (the
  evidence window survives a lapse),
  [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed conformance)

## Context

ADR-0041 made cross-cutting work declarable: a task names, in `also_serves`, the
additional goals it serves, and work spanning them reads as reviewable breadth
rather than drift. Coverage was fixed at creation and given no later mutator,
for a good reason recorded in the code itself — coverage added once conformance
has complained is a rationalisation, not a plan.

The rule was right about the failure it prevented and wrong about when it binds.

Goals attach to **files**, not to subject matter. An agent therefore does not
know the governing set when it creates a task; it learns it while working, when
it touches a file nobody predicted. `advise` reports this accurately, but the
answer arrives after the only moment the agent was allowed to act on it.

Measured on 2026-07-30, on the change that moved the VS Code engine floor
(ADR-0073): one unit of work took **three task creations**.

1. The first task served one goal. `advise` over six artefacts returned `review`
   — two files were governed by a second objective. Coverage could not be
   amended, so the task was replaced.
2. The second declared both. The finished change touched **fifteen** files, not
   six. `Makefile` — via a four-line comment the change itself made stale — is
   governed by a third goal. The receipt came back `drift`.
3. The third declared all three, and was refused: *"evidence interval falls
   outside the live claim"*. A claim opened after the work cannot own that
   work's evidence.

Step 3 is the important one, and its refusal is correct: re-claiming to collect
a cleaner verdict is exactly the manufactured receipt this plane exists to
prevent. But combined with fixed coverage it left the agent with **no honest
move at all** — only shipping a drift receipt, or reverting a file to buy a
clean one. In that instance reverting was itself dishonest: the stale comment
had been made false *by* the change, so dropping it would have left a false
statement in the repository in exchange for a green receipt.

A rule that leaves no honest move does not prevent dishonesty; it just makes
honesty expensive, and expensive honesty is what erodes.

## Decision

Coverage may be declared on a task **while its claim is live**, and is refused
once **any conformance record exists** for that task.

The boundary moves from *creation* to *the first verdict*. The distinction was
already named in the original rationale — "a rationalisation for a finding
**already raised**". Before any finding, a declaration is still a prediction the
evidence can contradict, which is the whole property that made declared coverage
trustworthy. After one, it is an excuse for that finding.

Four properties make the boundary hold:

- **Owner-guarded and claimed-only.** What a claim serves is the statement of
  the agent holding it. A task that is not `claimed` by the declaring agent is
  refused.
- **Union, never replacement.** A declaration adds goals and can never drop one
  already declared, so naming what you just learned cannot quietly erase what
  you knew at the start.
- **Logged.** A declaration appends a `coverage_declared` task event naming the
  goals and the agent. A task that grew its scope must show when and by whom; a
  wider claim with no history is indistinguishable from a rewritten one.
- **No second route.** Work whose verdict was not `aligned` lands in
  `in_review`, which is absent from the claim predicate. It cannot be re-claimed,
  so it cannot be widened and re-judged. This is pinned by test rather than left
  as an assumption about a SQL `WHERE` clause.

No new tool verb. The declaration rides on `task_claim`, which is already where
a task says what it expects to touch, and a same-owner re-claim already keeps
the evidence window open (ADR-0048) — so declaring costs nothing and opens no
window that cannot own its own work.

## Consequences

The agent who learns mid-change that a third goal governs one of its files can
now say so, and the receipt reports reviewable breadth instead of drift. The
remedy that was previously required — abandon and recreate — is not merely
discouraged but demonstrably broken after the first commit, so removing the need
for it removes a trap rather than a discipline.

Nothing that could previously earn `aligned` earns it more easily. A declaration
made before any evidence is judged still has to survive the judgement: declared
coverage caps at `needs_human` rather than passing silently (ADR-0041), so
breadth is reviewed, not waved through.

The cost is that "coverage is immutable" is no longer true as stated, and two
source comments asserting it have been corrected. A comment that contradicts the
code is worse than no comment, and this one had already been read as describing
a principle when it was describing a mechanism.

This does not remove the reason to run `advise` over the **full** file set before
creating a task. Declaring late is a recovery, not a plan; the receipt still
records that the task's scope grew, and a reviewer can still ask why it was not
known earlier.

## Alternatives considered

**A dedicated `task_cover` verb.** Rejected: it adds surface area (ADR-0059) for
an act that belongs to the claim, and it would need to re-derive the ownership
and lifecycle guards `task_claim` already applies.

**Allow coverage until completion rather than until the first verdict.**
Rejected: `check_conformance` can be run repeatedly, so the agent could read a
`drift` finding, widen coverage, and re-check — the rationalisation the original
rule was written to stop, reached by a slightly longer path.

**Infer coverage automatically from the files touched.** Rejected: it would make
every change self-covering and delete the signal entirely. Declaring breadth is
supposed to be an act someone is accountable for.

**Leave it, and require abandon-and-recreate.** Rejected on measurement: it does
not work after the first commit, because the replacement claim cannot own the
evidence. The guidance that existed was not merely awkward, it was unachievable.

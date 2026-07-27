# ADR-0041: Cross-cutting work is declared, not waived

- Status: Proposed
- Date: 2026-07-27
- Deciders: MindLeak maintainers
- Related: [ADR-0009](0009-evidence-backed-conformance.md) (conformance),
  [ADR-0029](0029-ask-before-acting.md) (advise),
  [ADR-0034](0034-typed-controls-and-enforcement-ceilings.md)
  (enforcement ceilings),
  [ADR-0039](0039-waivers-end-amendments-change.md)
  (waivers end; changing the rule is an amendment)

## Context

A task serves exactly one goal. Conformance resolves every changed node's
`governed` bindings and buckets them: a binding to the task's own goal is
`in_scope`, and a binding to any other goal is `other`. A non-empty `other`
returns `Drift`.

That rule is correct for its intended case — an agent editing governed code that
no covering task sanctions. It is wrong for a case it cannot distinguish from
it: work that legitimately serves several goals at once.

Three instances are now on record, all in this repository:

| Change | Task goal | Also touched, governed by |
|---|---|---|
| ADR-0024 preflight overlap | intent plane | graph, principled delivery |
| ADR-0018 isolated hooks | graph | principled delivery |
| ADR-0035 session context | intent plane | graph, ADR-0030 identity |

The ADR-0035 case is the clearest. `open_session` is one behaviour exposed by
two planes over one shared registry. `crates/mindleak-mcp/src/tools/mod.rs` is
bound to the graph goal because it is the graph engine's MCP surface, and to the
ADR-0030 identity goal because it registers sessions. Both bindings are correct.
There is no mislabelling to unpick, and the change cannot be made in one plane
only — a shared crate's signature change that compiles in one plane and not the
other is not a smaller commit, it is a broken one.

Today all three land as `Drift`, with a finding that reads
*"governed code changed without a covering task"*. That is the same signal an
unsanctioned edit produces. The audit cannot tell honest breadth from drift, so
the verdict stops meaning anything: the reviewer learns only that goals were
crossed, never whether crossing them was the point.

## Decision

**A task may declare, at creation, the additional goals it serves.** Conformance
treats a `governed` binding to any declared goal as in scope.

Four properties make the declaration honest rather than an escape hatch.

1. **Declared up front, immutable after.** Coverage is fixed when the task is
   created and can never be added later. This is the whole safety property. The
   failure mode worth designing against is not breadth — it is an agent that
   hits `Drift`, then widens coverage until the complaint disappears. Declaring
   before the work makes coverage a prediction that the evidence can contradict;
   declaring after makes it a rationalisation.

2. **Declared breadth cannot self-certify.** Under the ADR-0034 ceiling rule, a
   task that reaches `aligned` *only because* of declared secondary coverage
   caps at `needs_human`, landing in `in_review`. If the evidence would have
   been aligned under the primary goal alone, the declaration changed nothing
   and the verdict stands. Declaring coverage buys an honest review, never a
   free pass — an agent's own assertion about its own breadth is exactly the
   ADR-0015 false-safety shape if it is allowed to certify itself.

3. **The audit names the declared goals it relied on.** A reviewer sees which
   secondary goals the verdict depended on, so "this task crossed into the graph
   goal, as declared" is distinguishable from "this task crossed into the graph
   goal, unannounced".

4. **Absent declaration changes nothing.** A task that declares no additional
   goals behaves exactly as it does today, including returning `Drift`.

## Why not the alternatives

**Split the task by governing goal.** This was the standing recommendation after
ADR-0024, and two further instances have shown why it does not hold. It assumes
the goal span is known at promotion time, when it is routinely discovered while
working. Worse, it forces a coherent change into commits that are individually
broken: half a shared signature change does not compile. AGENTS.md forbids
shipping "the right design plus a hack to make CI green", and a deliberately
non-compiling commit is that hack wearing a process costume.

**Waive it.** ADR-0039 settled this: every waiver ends, and changing the rule is
an amendment. A shape that recurs and is legitimate every time is not a series
of exceptions — it is a rule that does not fit. Renewing a waiver each time
cross-cutting work appears would convert a modelling gap into permanent
paperwork, and eventually into a reflex nobody reads.

**Relink the goal, or narrow the evidence to dodge the finding.** Both make the
audit lie about what changed. DEVELOPERS.md already calls this out by name.

**Drop or soften the check.** The check catches real unsanctioned edits. The
defect is that it cannot express legitimate breadth, not that it is too strict.

## Consequences

- `Drift` regains its meaning: governed code changed that *nothing* sanctioned.
- Cross-cutting work becomes visible as an up-front claim rather than an
  after-the-fact argument, and the breadth of a task is reviewable before any
  code is written.
- Cross-plane tasks stop being a trap that strands honest work in `in_review`
  with a finding that reads like an accusation.
- Tasks that declare coverage still require a human to accept them. That is more
  friction than a single-goal task, deliberately: breadth is the thing a
  reviewer should look at.
- A promoter can still over-declare. That is bounded by properties 2 and 3 — it
  cannot buy an `aligned`, and it is recorded — but it is not prevented, and no
  mechanism here pretends otherwise.

## Enforcement and test plan

Platform-agnostic (`cargo` / `npm` / `node` / `git` only):

1. **Undeclared is unchanged.** A task with no declared coverage that touches
   another goal's governed code still returns `Drift`.
2. **Declared coverage is in scope.** The same evidence, under a task that
   declared the other goal at creation, no longer returns `Drift`.
3. **Declared coverage cannot self-certify.** That task returns `needs_human`,
   not `aligned`, and the findings name the declared goals relied on.
4. **Coverage that changed nothing is silent.** A task that declared a goal its
   evidence never touched reaches the verdict it would have reached anyway.
5. **Coverage is immutable.** There is no verb that adds coverage to an existing
   task; declaring an unknown goal, or the task's own primary goal, is refused
   at creation.

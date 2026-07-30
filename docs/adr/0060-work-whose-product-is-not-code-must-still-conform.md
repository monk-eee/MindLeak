# ADR-0060: Work whose product is not code must still be able to conform

- Status: Accepted
- Date: 2026-07-29
- Related: [ADR-0009](0009-evidence-backed-conformance.md) (evidence-backed
  conformance),
  [ADR-0019](0019-task-retention-and-board-hygiene.md) (task retention and board
  hygiene),
  [ADR-0057](0057-work-already-done-is-a-collision.md) (work already done is a
  collision),
  [ADR-0058](0058-work-that-shipped-must-leave-the-board.md) (work that shipped
  must leave the board),
  [ADR-0059](0059-the-tool-surface-is-a-vocabulary.md) (the tool surface is a
  vocabulary)

## Context

Conformance (ADR-0009, ADR-0025) decides whether the evidence an agent produced
actually served the goal its task was bound to. The check ends with two rules
that look symmetric and are not:

```rust
let touched_task_goal = !governing.in_scope.is_empty();
let Some(task) = task else {
    findings.push("no governed code touched".to_string());
    return Ok(ConformanceResult { verdict: Verdict::Aligned, findings });
};
if !touched_task_goal {
    findings.push("evidence does not touch code bound to the task goal".to_string());
    return Ok(ConformanceResult { verdict: Verdict::NeedsHuman, findings });
}
```

Evidence that touches no governed code with *no* task attached is `aligned`.
The same evidence *with* a task attached is `needs_human`. The presence of a
task makes the verdict worse.

The consequence is not an edge case. A task whose product is documentation, an
ADR, a benchmark, a changelog fragment, or a build script can never reach
`aligned`, because none of those artefacts are bound to a goal by
`link_goal_to_code` — that verb binds code. The agent does the work correctly,
produces well-formed evidence with real commit provenance, and the ledger parks
the task in `in_review` awaiting a human who has no queue to watch.

### Measured on this repository

A census of every task on the board with at least one conformance audit
(`target/tmp/audit-census.mjs`, 169 tasks, 90 audited):

| latest verdict | tasks |
|---|---|
| aligned | 45 |
| needs_human | **34** |
| drift | 11 |

The 34 `needs_human` verdicts have exactly two causes, and neither is human
judgement:

| latest finding | tasks |
|---|---|
| evidence contains no provenance-bearing mutation | **24** |
| evidence does not touch code bound to the task goal | **10** |

The 24 are the `ingest_commit` argument-drop defect: `files` was silently
dropped because the parameter is named `changed_files`, so no `refactored`
edges were written and the evidence had no mutation to point at. That cause is
fixed at the source — unknown arguments are now refused rather than dropped —
but the tasks it stranded remain stranded.

The 10 are this ADR's subject. One of them is the task that produced this ADR's
own supporting measurement: evidence with one commit, one changed artefact, and
complete `observed`/`refactored` provenance, parked because the artefact was
`DEVELOPERS.md`.

So **38% of all audited work is parked for a structural reason**, and the
board's 61 abandoned and 21 claimed tasks are downstream of that: an agent whose
correct work cannot be marked done either abandons the task or walks away from
it still claimed. The zombie claims are a symptom; this is one of the causes.

## Decision

1. **A task is judged against what it promised, not against whether it touched
   code.** `needs_human` must mean "a human needs to look at this", not "the
   work product was not Rust". Evidence that satisfies a task's acceptance
   criterion conforms regardless of the file extensions involved.

2. **Not touching goal-bound code is a fact to record, not a verdict to fail
   on.** The finding stays — it is genuinely useful to know that a task bound to
   a code goal produced no code. But on its own it resolves to `aligned` with
   that finding attached, matching the no-task branch directly above it. Only a
   *positive* signal of a problem — drift, a `forbid_change` lock, missing
   provenance, governed code changed without a covering task — may downgrade a
   verdict.

3. **Goals may bind non-code artefacts.** `link_goal_to_code` binds code because
   that is what it was built for. A goal whose delivery includes documentation,
   ADRs, or benchmarks can bind those artefacts too, so `touched_task_goal` is
   answerable for them rather than vacuously false. The verb's name becomes
   wrong; that is a rename, not a redesign.

4. **The claim's evidence window must be readable from the tool surface.**
   `complete_task` rejects evidence whose `started_at` precedes
   `claim_started_at`, and no tool returns `claim_started_at` — `claim_task`
   returns only `governing` and `won`, and `task_scope` returns only `paths` and
   `symbols`. An agent cannot construct a valid evidence window without
   guessing, and a wrong guess reads as "evidence interval falls outside the
   live claim", which sounds like a policy refusal rather than a missing
   accessor. `claim_task` returns the window it opened.

5. **Retrospective conformance stays the backstop.** Nothing here softens
   ADR-0009 or ADR-0025. Drift is still drift and a violation is still a
   violation; this removes one false negative, it does not remove the check.

## Consequences

Correct non-code work can be completed, which is the point. The board stops
accumulating parked tasks for a reason no human will ever adjudicate, and
`in_review` regains its meaning: something a person actually has to decide.

The 10 currently parked on this finding do not fix themselves. They were
audited under the old rule and their verdicts are historical records, which
ADR-0025 says we do not rewrite. They need re-auditing under the new rule or
explicit disposition — the same problem ADR-0058 raises for work that shipped
without leaving the board, and it should be solved once for both.

We lose a signal we were not really using. A task bound to a code goal that
produces no code may well be mis-scoped, and the old rule surfaced that by
stopping. It still surfaces it, as a finding on an aligned audit — visible in
`conformance_history`, and cheap to query for. That is the right weight for a
smell: recorded, not blocking.

Renaming `link_goal_to_code` touches every caller and the tool surface, which is
a breaking change to an advertised verb. Under ADR-0059 that is a vocabulary
change and gets the same treatment as any other: migrate the callers, do not
ship an alias beside it.

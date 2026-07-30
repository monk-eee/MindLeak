- **The pre-flight could not see the coverage the gate reads, and advised
  against the one shape the constitution provides for cross-cutting work.**
  `advise` resolved governing clauses from the task's own `goal_id` alone, while
  `evaluate_base_conformance` resolves them through the task's recorded
  `goal_coverage` (ADR-0041). The shared resolver exists precisely so the
  forward-looking advice and the retrospective gate cannot fork the rule — its
  own doc comment says so — and the `advise` call site forked it by passing no
  coverage.
  So a task that had correctly declared the governing goal in `also_serves` at
  creation was still told its change *"would drift; get a covering task or
  review before acting"*, while the gate it was predicting would have found it
  in scope.
  Wrong in the most expensive direction. An agent that believes the advice
  re-declares the task; the replacement carries the same coverage; the answer
  does not change. `also_serves` is fixed at creation with no verb that adds
  coverage later, so the advice invites a loop with no exit — measured live on
  2026-07-30, where a correctly covered replacement task was told the same
  thing as the one it replaced.
  `advise` now resolves through the same coverage. A task without the
  declaration still reads as drift, so this is coverage-aware rather than a
  blanket softening, and the test asserts both directions.

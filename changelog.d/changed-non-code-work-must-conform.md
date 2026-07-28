- **ADR-0060 proposes that work whose product is not code must still be able to
  conform.** Conformance ends with two rules that look symmetric and are not:
  evidence touching no governed code with no task attached is `aligned`, while
  the same evidence with a task attached is `needs_human`. Attaching a task
  makes the verdict worse, so a task whose product is documentation, an ADR, a
  benchmark, or a build script can never reach `aligned`. Measured across this
  repository's 169 tasks (90 with an audit): 45 aligned, 34 `needs_human`, 11
  drift — and the 34 have exactly two causes, neither of them human judgement.
  24 are the `ingest_commit` argument-drop defect and 10 are this rule, so 38%
  of audited work is parked structurally.

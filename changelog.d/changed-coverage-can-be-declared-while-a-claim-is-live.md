- Goal coverage can now be declared while a claim is live, and is refused once
  conformance has judged the task (ADR-0074). `also_serves` was fixed at task
  creation, on the sound reasoning that coverage added after conformance
  complains is a rationalisation. But goals bind to files, so the governing set
  is learned while working, not predicted at creation — and after the first
  commit the previous remedy did not work at all: a task created to gain
  coverage cannot own the earlier work's evidence
  ("evidence interval falls outside the live claim"). Measured on 2026-07-30,
  one change took three task creations and still shipped a drift receipt.
  The boundary moves from creation to the first verdict, which is the
  distinction the original rationale already named — a rationalisation is for a
  finding *already raised*. Before any finding, a declaration is still a
  prediction the evidence can contradict.
  Declaring is owner-guarded and claimed-only, unions rather than replaces so it
  can never drop a goal declared earlier, and appends a `coverage_declared` task
  event so a task that grew its scope shows when and by whom. Work already in
  `in_review` cannot be re-claimed, so a verdict cannot be widened and re-judged.
  No new tool verb: the declaration rides on `task_claim`, which already says
  what a task expects to touch, and a same-owner re-claim keeps the evidence
  window open.

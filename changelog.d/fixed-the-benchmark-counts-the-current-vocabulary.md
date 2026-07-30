- **The agent-loop benchmark stopped counting the thing it exists to measure.**
  `summarizeEvents` classifies each tool call the agent under test actually
  made, and for the Intent Plane it matched
  `/(constitution|board|next_task|active_knowledge)/`. ADR-0059 collapsed that
  vocabulary, so the server the agent talks to now advertises `task_query`,
  `task_create`, `task_claim` and `task_transition` — names the classifier
  could not match. Every coordination call the agent made was silently dropped
  from the exploration count.
  Nothing failed, and nothing could: a name-keyed classifier has no way to
  report that it stopped matching. It returns `false`, the run completes, and
  the exploration and cost figures simply come out lower and look ordinary.
  Every agent-loop run since the collapse under-counted the **mindleak+lodestar
  arm** — the one arm the benchmark exists to justify.

  The classifier now recognises the collapsed verbs **and keeps the retired
  ones**. That is deliberate: `benchmarks/results/2026-07-22-agent-loop-outcome.json`
  was measured before the collapse, when the agent could only call the old
  names, so dropping them would have re-defined the metric rather than repaired
  it. Keeping the change a superset is what makes this a fix to a counter
  instead of a silent re-baselining, and it means the published result stays
  comparable to future runs.

  The classifier moved into `scripts/agent-loop-events.mjs` so it can be
  tested at all — `evaluate-agent-loop.mjs` spawns the Copilot CLI at import
  time, so any test importing it would have started a real four-arm evaluation,
  which is why this code had no test and why the rot went unnoticed. A
  synthetic event stream in the current vocabulary now asserts those calls are
  counted; restoring the old pattern fails it, naming `task_query`.

- **The fleet's own publish path calls tool names the removal train will delete
  — MEASURED 2026-07-30, OPEN.** ADR-0059 collapsed the design cluster (15 → 4)
  and the task cluster (26 → 4), and kept the old names answering for one minor
  version. Measured against `origin/main`, the committed scripts still name
  **7 retired verbs across 17 call sites in 11 of 55 scripts**. Five of those
  are real tool calls that stop working when the aliases are removed:
  - `scripts/canonical-push.mjs:227` (`board`) and `:228` (`check_overlap`)
  - `scripts/board-health.mjs:298` (`board`)
  - `scripts/stranded-report.mjs:211` (`board`)
  - `scripts/design-audit.mjs:188` (`list_designs`)
  - `scripts/evaluate-agent-loop.mjs:448` (`create_task`), `:459` (`claim_task`)

  `canonical-push.mjs` is the **only sanctioned way to publish**, and it runs
  from a pre-push hook rather than from a UI anyone can route around. When the
  removal lands, publishing stops for every agent at once, and the failure
  arrives as a tool-not-found from inside a git hook rather than anywhere that
  names the cause. This is a larger blast radius than the extension migration
  recorded above, and it is the same window.
  — Impact: the removal train currently breaks the fleet's ability to publish
  and to report on its own board. — Not fixed here: this commit fixed the
  guidance strings and the guards, which are safe to change in isolation.
  Repointing `canonical-push` is a change to the publish path itself and wants
  its own reviewed commit with a live rehearsal, not a rider on a message fix.

- **The agent-loop benchmark measures the deprecated surface —
  MEASURED 2026-07-30, OPEN.** `scripts/evaluate-agent-loop.mjs` drives
  `create_task` and `claim_task` (`:448`, `:459`), and classifies the resulting
  events with `/(constitution|board|next_task|active_knowledge)/` (`:691`) — a
  name-keyed regex. Both halves are self-consistent, so nothing fails; the
  benchmark simply reports numbers about the vocabulary agents are being moved
  off, while the surface it is meant to characterise is `task_create`,
  `task_claim`, `task_query` and `task_transition`. A name-keyed classifier
  cannot report that it stopped matching — it returns `false` and the run looks
  ordinary. — Impact: agent-loop results describe the pre-ADR-0059 surface and
  will silently keep doing so until the aliases are removed, at which point the
  benchmark breaks rather than corrects itself. — Not fixed here: changing what
  the benchmark drives changes what its published baselines mean, so it needs
  to be re-baselined deliberately (`benchmarks/results/`) rather than edited.

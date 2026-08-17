- **The Lodestar default tool profile has real headroom again.** It was
  measured at ~5,976 of its 6,000-token ceiling (ADR-0059), leaving no room
  for the next legitimate schema addition. `task_query`, `task_transition`,
  `task_claim`, and `task_create` had their descriptions trimmed to the
  operational contract only — every branch and argument a caller needs, with
  the design rationale moved to [ADR-0093](docs/adr/0093-tool-descriptions-are-a-contract-not-a-narrative.md)
  and `docs/TOOLS.md` instead of being repeated in the wire schema every
  session. The profile now measures ~5,383 tokens, and
  `the_default_profile_is_under_budget` gains a tighter headroom assertion
  (under 5,500 tokens) so the next addition that would re-saturate the budget
  fails a test instead of silently spending the margin.

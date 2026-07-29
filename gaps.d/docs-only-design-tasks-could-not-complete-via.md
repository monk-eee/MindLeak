- **Docs-only design tasks could not complete via conformance, stranding
  successors — FIXED.** — A design task produces a docs commit; `complete_task` runs
  ADR-0009 code conformance, which returns `needs_human` ("evidence does not touch
  code bound to the task goal") and parks the task in `in_review` forever. Any
  implementation task chained `blocked_by` a docs-ADR predecessor then never opens
  (`blocked_by` clears only on predecessor `done`), and with no live `reopen_task`
  it cannot be un-gated — clearing the gate via `block_task(id, None)` leaves it
  `blocked` with no predecessor and no path back to `open`. — High impact on the
  design-first workflow. — Fixed for registered *design items* by the accepted
  ADR-0023 Design Board path: a human `accept_design` completes design review
  without code conformance, then a separately reviewed create/link/no-work plan
  maps it to executive work. Blind fallback creation was removed after ADR-0028
  exposed a duplicate-task failure. A docs-only task inside an *objective's*
  task chain (not a registered design item) —
  e.g. the AGENTS.md/README/USAGE/SPEC-INTENT task closing the ADR-0029 advise
  chain — still lands `in_review` via the same honest `needs_human` verdict. — **Fixed
  Jul 2026:** `resolve_task(task_id, human)` (facade + MCP) is the task-level
  mirror of `accept_design` — it human-accepts an `in_review` task to `done` with
  no code-conformance re-run while preserving the original audit, opens any
  blocked successor, and refuses self-resolution by the reviewed agent (the
  worker read from the task's conformance evidence). `reopen_task` and
  `abandon_task` retain their distinct recovery and retirement meanings. Tests:
  `resolve_task_accepts_an_in_review_task_to_done`,
  `resolve_task_refuses_self_resolution_by_the_reviewed_agent`,
  `resolve_in_review_opens_a_blocked_successor`.

- **The prose docs now speak the current tool vocabulary.**
  The task-lifecycle and design clusters collapsed into four verbs each
  (`task_create` / `task_claim` with `step` / `task_transition` with `to` /
  `task_query` with `view`, and `design_register` / `design_decide` /
  `design_promote` / `design_query`), but USAGE, SPEC-INTENT, WALKTHROUGH,
  QUICKSTART, ARCHITECTURE, TOOLS, SPEC-CONSTITUTION, AGENTS, and the extension
  README still called the retired names — actively misdirecting any agent
  reading them. Every worked flow, the §9 wire-tool contract, and the inline
  references now name the current verb and its real argument shape (e.g.
  `complete_task(...)` → `task_transition(task_id, to="complete", ...)`,
  `next_task()` → `task_query(view="next")`, `renew_lease(...)` →
  `task_claim(task_id, step="renew", ...)`). References to the Rust **facade
  methods** (e.g. `Lodestar::complete_task` in ARCHITECTURE) and to `task_query`
  **view** names are unchanged, because those are current — only the client-facing
  MCP tool names had moved.

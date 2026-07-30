- **The VS Code extension no longer depends on Lodestar's deprecated tool-name
  compatibility window.** Design workflows now call `design_register`,
  `design_decide`, `design_promote`, and `design_query`; task workflows call
  `task_claim`, `task_transition`, and `task_query`, with the former verb
  encoded explicitly as `step`, `to`, or `view`. The migration covers board
  refresh, evidence completion, question handling, lease changes, overlap
  checks, and every Design Board operation while leaving MindLeak's separate
  `check_overlap` tool unchanged.
  A TypeScript-AST regression audits every Lodestar `callTool` site, rejects
  retired aliases and dynamically constructed tool names, and verifies that
  each clustered call carries its discriminator. This includes the former
  runtime-only `` `${action}_task` `` pause/resume path that literal searches
  could not see.

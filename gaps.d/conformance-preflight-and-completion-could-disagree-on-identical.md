- **Conformance preflight and completion could disagree on identical evidence.**
  — `check_conformance` returned `aligned` for task `task:aae950aecd78`, then
  `complete_task` immediately reran the optional semantic judge, returned
  `needs_human`, and stranded the task in review despite no evidence or intent
  change. — High impact on verified delivery. — Resolved Jul 2026 by ADR-0025:
  checks now return a durable id + state token, and completion consumes that
  exact audit result without a second model call (task `task:1b5bdafd5e99`).

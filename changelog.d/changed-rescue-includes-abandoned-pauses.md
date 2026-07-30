- **A paused task whose owner disappears now reaches new agents after its
  seven-day protection grace.** `open_session` keeps healthy pauses private to
  their owner, then includes overdue paused work in `rescue_work` with the
  former owner, branch, and canonical scope/claim actions once normal recovery
  is allowed. Reading the queue never transfers or mutates the task.
  Core and MCP regressions pin both sides of the boundary so deliberate short
  pauses stay quiet while abandoned pauses cannot remain invisible forever.

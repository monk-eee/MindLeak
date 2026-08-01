- **Re-claiming a task no longer erases its evidence scope by omission.** An
  omitted `paths` or `symbols` field now preserves that part of the task scope
  atomically. Explicit arrays still replace the selected field, and `[]`
  remains the deliberate way to clear it, so rescued work retains the linkage
  `merge_evidence` needs without making scope impossible to revise.

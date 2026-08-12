- **`task_create` now names an exact-title duplicate instead of leaving it to be
  found.** The answer already reported what served the goal, but it reported it
  as `prior_work` — every task ever created under that goal, which ran to 203
  entries on this board — so the one line that mattered could not be spotted by
  reading it. Two agents created "Make worktree reclaim refuse loudly when the
  Lodestar board is unreadable" against a single goal with that report in front of
  them. Live work carrying the exact title just created is now carried separately,
  in `duplicates`, with its id, status and owner. It still creates the task: a
  second task against one goal is often right under ADR-0015, and the point is
  that you find out now rather than once two agents have claimed both.

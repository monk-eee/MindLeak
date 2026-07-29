- **Two tasks with the same title on the same goal in the same second fail with
  a raw SQLite error — FOUND, not fixed.** A task id is
  `task:{short_hash(goal_id|title|now)}` where `now` is whole seconds
  (`create_task_after_on`, `crates/lodestar-core/src/store/coordination.rs`).
  Create the same title twice within one second and the second call returns
  `sqlite error: UNIQUE constraint failed: tasks.id` — an implementation detail
  leaking as an error message, for what is either a legitimate retry or an
  obvious duplicate. Hit twice while writing the `existing_work` tests, which is
  the only reason it was noticed; the six real duplicates on the board landed
  seconds apart and so slipped through. Impact: confusing failure for scripted
  task creation, and a de-duplication rule that exists by accident, applies for
  one second, and reports itself as a database fault.

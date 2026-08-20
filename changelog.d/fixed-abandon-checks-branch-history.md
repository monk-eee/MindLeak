- `abandon` now refuses (without `acknowledge_branch=true`) whenever a task's
  event history ever recorded any branch, not only the branch currently on
  the row. A claim that rescues a lapsed lease legitimately re-reads the
  branch for its own fresh window, which discards an earlier owner's branch
  from the live column — exactly the value `abandon` needs to check for an
  open or merged pull request. That value was never lost: every claim already
  records the resulting task as an event (ADR-0064), so the discarded branch
  survives in `task_events` even after a later claim moves the live column
  past it. The refusal now names every distinct branch the history has ever
  seen.

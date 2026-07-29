### Added

- **`existing_work` answers whether this has already been done.** Six identical
  "carry controls across an amendment" tasks and four identical "run the merge
  queue ourselves" tasks reached the board because nothing could answer that
  question: `check_overlap` reports who is touching a file *right now*, and
  `board` hides finished work — so completed and abandoned work, the answers
  that matter most, were invisible. `existing_work(goal_id | paths)` returns
  the tasks already serving a goal or already declaring those paths in their
  scope, terminal states included. Path matching reuses `check_overlap`'s glob
  comparison so the two cannot drift, and asking about nothing is refused
  rather than answered "nothing exists" — a clean bill of health for a question
  never asked is the failure this exists to prevent.

  `create_task` now names the prior work serving the same goal and still
  creates the task: a second task against one goal is often legitimate, and a
  gate here would be wrong more often than right (ADR-0015).

  Not yet answered: which branch that prior work is on, and whether it is
  merged. `Task` has no branch field — that is a separate open task, and
  reporting a branch before one is recorded would be a guess.

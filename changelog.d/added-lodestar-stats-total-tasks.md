### Added

- `lodestar_stats` reports `total_tasks` -- every task ever created, any
  status -- alongside its existing goal/task/knowledge counts, so a
  `board`/`stalled` caller seeing an empty result can tell "nothing has
  ever been created here" apart from "everything is genuinely clean"
  without guessing. `open_tasks + claimed_tasks + done_tasks` was not that
  number: it silently omitted blocked, in_review, and abandoned tasks. See
  `gaps.d/board-and-stalled-cannot-distinguish-empty-from-never-used.md`.

- **A newer writer no longer blinds an older reader.**
  The task event log is append-only and shared by binaries of different
  vintages — an agent running yesterday's build reads a database today's build
  writes to. The reader refused the whole read on any kind it could not name, so
  the first write of a new kind bricked every older reader at once. Observed
  live: hours after `coverage_declared` first landed, every
  `task_query view=board` call on the installed binary returned
  `invalid: unknown task event kind: coverage_declared` instead of the board,
  because the board tool enriches every row with `claim_window`, which reads the
  log. One event nobody could name took the whole board down — and with it the
  Design Board's promote route, which reads the task board to offer existing
  work to link. An unnameable kind is now skipped rather than fatal, and named
  on stderr rather than swallowed, because the remedy is to rebuild and nobody
  rebuilds to fix a symptom they were never shown. `project_tasks` now reads
  after-images directly instead of going through the typed log: the replay never
  inspects `kind`, and inheriting the skip would drop a task's latest state and
  make the ADR-0064 projection check report a hole in the log that is not there
  — a false accusation that a transition went unrecorded, when the only real
  fault is that the reader is older than the writer.

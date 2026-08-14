- **The delivery watcher runs the artefact sweep in a fresh process, so a
  long-running watcher can no longer delete by stale rules.** It previously
  imported `sweepIfDue` and `describeSweep` from `artefact-sweep.mjs` and called
  them in-process. Node loads a module once, at startup, and never re-reads it —
  but this watcher runs for days, so one started before a safety fix kept
  deleting by the rules it booted with, and nothing in Git could see it because
  the file on disk was already correct.
  Measured 2026-08-13: a watcher a day old deleted the fleet host's
  `editors/vscode/node_modules` that PR #435 had specifically taught the sweep to
  spare, leaving every agent's `prettier` pre-commit hook failing with
  `MODULE_NOT_FOUND`. The freshness guard added in PR #453 could not help,
  because a process that never re-reads its source never reaches the guard.
  `delivery-queue.mjs` now spawns `scripts/artefact-sweep.mjs --if-due` (adding
  `--apply` only when the queue is applying) and imports nothing from it. A child
  process reads the rules at the moment it uses them, so the age of the watcher
  stops being a variable.
  `artefact-sweep.mjs` gains `--if-due`, which obeys the persisted cadence
  instead of forcing, and prints nothing when a sweep is not due — the answer to
  almost every unattended call. The watcher announces a sweep result only when it
  changes, so a persistent refusal is stated once rather than sixty times an
  hour.

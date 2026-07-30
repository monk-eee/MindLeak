- **The only sanctioned publish path called tool names the removal train will
  delete.** `canonical-push.mjs` asked for `board` and `check_overlap`;
  ADR-0059 retired both into `task_query` with `view="board"` and
  `view="overlap"`, and they answered only because the deprecation window
  answers them. Because canonical-push runs from a pre-push hook rather than a
  terminal anyone is watching, the removal would not have surfaced as a tidy
  error: publishing would have stopped for **every agent in the fleet at the
  same moment**, as a tool-not-found from inside git, naming neither the cause
  nor the fix. `board-health`, `stranded-report` and `design-audit` followed the
  same path. All four now speak the current vocabulary, proven by publishing
  this change through the migrated push rather than by reading the diff.

- **A guard now refuses a retired name in the delivery scripts, and names the
  file and line.** Migrating five call sites lasts until the next rename; the
  point of the collapse was to stop finding this class by hand, and it has now
  been found by hand four times — dispatch, `requires_session`, a test
  whitelist, and the publish path. The new check reads the delivery scripts and
  fails with `scripts/<file>:<line> calls <name>`, which is the whole value: it
  arrives before the push instead of inside a hook, and it says where.
  It reads **tool-name positions only**, never argument values. A collapsed
  verb takes the retired name *as* an argument — `task_query` with
  `view: "board"` — so a scan matching the bare quoted string would report the
  migration itself as a violation, and a guard that cries wolf is one people
  learn to skip. That is precisely how the guards it replaces went stale, so
  the check also asserts it can still see the live call sites: a scan that
  reads nothing passes, and passing is indistinguishable from working.

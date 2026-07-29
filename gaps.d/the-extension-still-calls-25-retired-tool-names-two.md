- **The VS Code extension still calls 25 retired tool names, two of which no
  search for a literal name can find — MEASURED 2026-07-30, OPEN.** ADR-0059
  collapsed the design cluster (15 tools → 4) and the task cluster (26 → 4).
  The extension was not migrated with them: measured against `origin/main`,
  `editors/vscode/src` names **25 retired verbs across 35 call sites in 7
  files** — `designBoardController.ts` (11), `taskAllocationController.ts` (9),
  `extension.ts` (6), `util.ts` (4), `readinessController.ts` (2),
  `boardViewProvider.ts` (1), `evidenceBoardViewProvider.ts` (1).
  Every one of them works **today**, and only because the deprecation window
  answers it. All of them break when the removal train named in ADR-0059
  arrives.
  Two of the twenty-five are the dangerous kind: `extension.ts:937` builds the
  tool name at runtime — ``callTool(`${action}_task`, …)`` where `action` is
  `"pause" | "resume"` — so `pause_task` and `resume_task` never appear as
  literals anywhere in the source. A grep for retired names finds 23 and
  reports the extension as nearly migrated; the two it misses are a live
  ownership transition. Any audit of "have we finished the migration?" that
  works by searching for names will answer *yes* while this line still runs.
  — Impact: the extension is entirely dependent on the deprecation window, and
  the completeness of its migration cannot be established by search. — Not
  fixed here: this is a 35-site migration across seven files with its own test
  surface, not a guard fix, and it wants to be scheduled against the removal
  train rather than folded into an unrelated commit.

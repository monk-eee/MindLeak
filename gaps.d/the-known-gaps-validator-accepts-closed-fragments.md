- **The Known Gaps validator accepts closed fragments and `make gaps` presents
  them as open -- MEASURED, OPEN.** The fragment contract is explicit in
  `scripts/gaps.mjs` and `DEVELOPERS.md`: closing a gap deletes its fragment, so
  the fix and removal are attributable in the same commit. The validator checks
  filenames and bullet shape only; it never checks whether the heading itself
  declares the gap `FIXED`, `RESOLVED`, or `CLOSED`.

  Measured on main `aa002af6e91060b5618addfa680e754ce4158eb2` on 2026-07-30:
  `gaps.d/` contains 76 fragments, 27 of whose bold headings carry one of those
  terminal markers, and `node scripts/gaps.mjs --check` still reports every
  fragment valid. Some of the 27 correctly retain an open residual, but at
  least these sampled entries are wholly terminal and name no remaining defect:
  `a-control-can-be-created-through-the-mcp.md`,
  `a-server-restart-could-strand-a-legacy-base.md`,
  `a-wrapped-status-line-silently-lost-its-reference.md`,
  `disposable-git-fixtures-inherited-the-parent-hook-s.md`, and
  `the-release-smoke-reported-success-on-platforms-it.md`. A previous cleanup
  commit (`9d7f170`) deleted 11 other closed fragments, confirming that deletion
  rather than historical retention is the intended lifecycle.

  Impact: the open-gap count is inflated, `make gaps` mixes completed history
  into actionable debt, and an agent cannot use the catalog to decide what work
  remains without rereading every fragment. Left open: classify all 27 headings,
  delete truly terminal fragments, rename partial fixes around the residual gap,
  and add a machine-checkable status rule so a later fix cannot leave its gap
  reading as open again.

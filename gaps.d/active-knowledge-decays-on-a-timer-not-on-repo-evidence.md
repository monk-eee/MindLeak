- **`active_knowledge` decays purely on `half_life_hours`, never on whether the
  code it describes has actually changed.** — Observed 2026-08-17: knowledge
  entries lose reach on a timer regardless of whether the ADR or file they
  reference has since been amended, superseded, or deleted. Where:
  `crates/lodestar-core/src/facade/knowledge.rs` (`record_knowledge`/
  `revise_knowledge` apply `decay::KNOWLEDGE_DEFAULT_HALF_LIFE_HOURS` with no
  check against current repo state). Impact: a lesson about a file can keep
  reaching agents for its full half-life even after the described behaviour
  was fixed (a false positive advisory), or can expire while the file it
  describes is untouched and the lesson still fully applies (a false
  negative). Left for later: no code change made this run; the fix shape is
  to let a knowledge entry's `node` references be checked against current
  repo state (git blame / file hash since recorded), rather than trusting
  elapsed time alone.

  **NARROWED 2026-08-26 — corrected a false citation, substantive claim
  re-verified and unchanged.** This entry originally pointed at
  `scripts/check-evidence.mjs` as prior art already doing this kind of
  evidence-backed staleness check. That file has never existed anywhere in
  this repository's git history (`git log --all --diff-filter=A -- "*check-
  evidence*"` returns nothing, on any branch, ever) — the comparison was
  wrong from the start, not a tool that was later renamed or removed. Left
  uncorrected, the next reader would go looking for a pattern to reuse and
  find nothing where the fragment said something existed. Re-verified the
  decay path itself directly against `facade/knowledge.rs` rather than trust
  the fragment's own account: the substantive claim holds exactly as
  written — decay is still purely timer-based, with nothing checking a
  knowledge entry's referenced nodes against current repo state. The fix
  shape described above is unbuilt and remains open; only the citation was
  wrong.

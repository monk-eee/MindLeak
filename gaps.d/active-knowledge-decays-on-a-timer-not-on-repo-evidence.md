- **`active_knowledge` decays purely on `half_life_hours`, never on whether the
  code it describes has actually changed.** — Observed 2026-08-17: knowledge
  entries lose reach on a timer regardless of whether the ADR or file they
  reference has since been amended, superseded, or deleted. Where:
  `crates/lodestar-core/src/facade/...` (knowledge promotion/decay path),
  compare with `scripts/check-evidence.mjs`, which already derives
  PROVEN/BROKEN/STALE for this repository's own claims from real code state.
  Impact: a lesson about a file can keep reaching agents for its full
  half-life even after the described behaviour was fixed (a false positive
  advisory), or can expire while the file it describes is untouched and the
  lesson still fully applies (a false negative). Left for later: no code
  change made this run; the fix shape is to let a knowledge entry's `node`
  references be checked against current repo state (git blame / file hash
  since recorded) the same evidence-backed way `check-evidence.mjs` already
  validates ADR claims, rather than trusting elapsed time alone.

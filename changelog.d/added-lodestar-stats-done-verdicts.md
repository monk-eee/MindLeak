### Added

- `lodestar_stats` reports `done_verdicts` -- a breakdown of every `done` task
  by aligned/needs_human/drift/violation/unresolved, taking each task's human
  `resolved_conformance_id` when present and otherwise its latest conformance
  check, or `unresolved` when no conformance record exists at all. `done`
  means shipped, not that conformance affirmed it; this was previously only
  answerable by a one-time manual audit (see
  `gaps.d/done-does-not-mean-aligned.md`), and is now queryable directly.

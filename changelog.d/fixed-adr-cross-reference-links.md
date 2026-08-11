- **Four ADR cross-references pointed at filenames that do not exist, and a
  check now catches the fifth.** ADR-0061, ADR-0062 and ADR-0065 each cited
  `0045-armed-means-finished.md`, and ADR-0078 cited
  `0034-a-control-is-not-a-guard-unless-it-can-refuse.md`. Neither file has ever
  existed: the decisions are filed as
  `0045-a-fleet-is-a-distributed-system.md` and
  `0034-typed-controls-and-enforcement-ceilings.md`. The ADR numbers in the link
  text were right all along — only the paths beside them were stale, which is
  the failure mode of a filename that carries a slug, because a slug is prose
  and gets reworded from memory while the number stays put. A broken link of
  this shape renders normally and fails only when a reader clicks it, which is
  why these four were found by writing a checker rather than by reading.
  `scripts/adr-link-check.mjs` now verifies every ADR cross-reference resolves,
  as a pre-commit hook scoped to the ADRs in the change.

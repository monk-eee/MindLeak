- **A link checker now guards the living documentation, and the one dead link
  it found is fixed.**
  `scripts/link-check.mjs` validates every relative markdown link in the living
  docs (README, AGENTS, DEVELOPERS, `docs/*.md`, the extension README) against
  the working tree, and its test runs from pre-push via `script-tests`, so a doc
  that starts pointing at a moved, renamed, or deleted file fails the push
  instead of rotting unnoticed. It resolves a target file-relative or
  root-relative (the repo mixes both), treats a directory target as valid, and
  exempts the `media/screenshots/` images the capture checklist tracks. It found
  AGENTS.md still pointing `GraphStore` at `graph.rs` after that module was split
  into `graph/`; the link now points at `graph/mod.rs`. `docs/adr/` is out of
  scope on purpose — an ADR's cross-references are historical, number-identified,
  and some point at decisions since renamed or never given their own file;
  repairing those is a maintainer's call about intent, tracked separately.

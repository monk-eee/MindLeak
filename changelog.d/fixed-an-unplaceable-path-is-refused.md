- Ingesting a file whose path cannot be made repository-relative is now refused
  instead of quietly creating a second identity for it. Every worktree of a
  repository shares one graph (ADR-0038), and `repo_relative` returns a path it
  cannot place untouched — correct for a helper, wrong for a node id — so a file
  saved in a sibling checkout arrived still absolute and became
  `artifact:c:/Users/.../MindLeak-build/crates/x.rs` alongside
  `artifact:crates/x.rs`. Splitting a file's identity splits everything derived
  from it: reinforcement decays corroborated signal like a one-off (ADR-0005),
  `check_overlap` never collides two agents on one file, a governance binding
  covers only one spelling, and recall returns the same file twice.
  Measured on the live graph on 2026-07-29:
  `crates/lodestar-mcp/src/tools/mod.rs` held 117 structural edges under its
  absolute id and 43 under its relative one. Those rows were unreachable rather
  than merely stale — `replace_structure` matches on the relative `owner_id`, so
  re-ingesting the file could never see them. `repair_workspace_paths` still
  merges duplicates that already exist; this stops new ones being made.

- **AST ingestion now preserves restricted Rust APIs, ignores non-code text,
  and keeps same-named methods distinct.** Declarations such as `pub(crate) fn`
  and `const fn` previously vanished, braces and call-shaped text in comments or
  literals could corrupt call edges, and methods in separate `impl` blocks
  collapsed onto one symbol id. Existing structural snapshots are marked stale
  for deterministic refresh under extractor version 2.

- **The mandatory pre-flight is bounded, so it can actually be read.**
  ADR-0066 put `check_overlap` on the before-you-write checklist and had it
  carry the impact radius. Two later changes landed on top of that — Rust
  `mod`/`use` extraction, which gave `.rs` files real cross-file structure for
  the first time, and a re-ingest pass that populated all of it at once. Each
  was right on its own; together they made the thing every agent is required to
  read too large to read.
  Measured 2026-07-29 on `crates/lodestar-mcp/src/tools/mod.rs`, a single path
  returned **196 nodes over 295 edges — 351 KB**, with 185 of those nodes
  carrying their full text. `impact_radius` traverses at zero minimum weight,
  depth two, with no node cap; that was harmless while Rust files had almost no
  cross-file edges, and stopped being harmless the moment they did.
  A decision aid that displaces the decision fails the same way an unread one
  does, which is the failure ADR-0066 was written to fix.
  The pre-flight now carries the 32 most relevant nodes without their content,
  only the edges among them, and `impact_total` so a trimmed answer cannot be
  mistaken for a complete one — the same reason `unknown` is reported separately
  from an empty impact. Ranking by traversal score keeps the cut meaningful
  rather than arbitrary. Same path, after: **47 KB**.
  The cap matches the existing hard cap on `working_set`, which bounds the same
  kind of view for the same reason. `get_impact_radius` is unchanged for callers
  that genuinely want the whole neighbourhood.

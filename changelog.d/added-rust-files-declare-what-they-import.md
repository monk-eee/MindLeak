- **Rust files now declare what they import, so impact can say what breaks.**
  The impact traversal is the deterministic half of MindLeak's memory — the part
  that answers "what depends on the file I am about to change" — and for Rust it
  had nothing to work with. Measured on this repository, the impact of a real
  `.rs` file (`crates/mindleak-core/src/facade/query.rs`) was 15 nodes over 15
  edges: its own commits, its own symbols, and not one other file, because Rust
  ingestion emitted no inter-file edges at all. Meanwhile `docs/EVALUATION.md`
  reported 1.00 precision on the impact question — measured on a **JS/TS**
  fixture where those edges exist. The benchmark and the experience were both
  honest and described different languages.
  Rust ingestion now recovers the module graph without compiling anything and
  without spending a token: `mod x;` resolves to the declaring module's
  directory (which for a non-root file is a directory named after the file, not
  the file's own directory — getting that backwards silently points every child
  module somewhere wrong), and `use crate::`/`self::`/`super::` resolve through
  a longest-first candidate ladder the store picks a known file from. That
  ladder is the same mechanism the JavaScript arm already used, reused rather
  than reinvented, because a `use` path cannot be split into module part and
  item part by looking at it: `crate::graph::GraphStore` and
  `crate::graph::query` are the same shape.
  Deliberately conservative where certainty runs out. Another workspace crate
  records as `package:<name>` rather than a guessed `crates/<name>/src/lib.rs`,
  because that mapping is a convention this code cannot verify; an inline
  `mod x { .. }` produces nothing, because no file is behind it; and comments
  and string literals are masked before parsing, so the prose in this
  repository's doc comments cannot fabricate an edge. All of these under-report,
  which is the safe direction — a missing edge is a smaller lie than an invented
  one.
  This is the follow-up ADR-0066 named: the pre-flight was put on the mandatory
  checklist with this limit stated rather than left unused, and closing it meant
  emitting the edges, not rewording the docs. `impact_radius` and the
  `check_overlap` pre-flight both return a Rust dependent end to end.

- **Rust impact traversal stops at cross-crate and nested-use boundaries —
   OPEN.** Run against a
  real file in this repository (`crates/mindleak-core/src/facade/query.rs`) the
  impact radius returned 15 nodes over 15 edges: 6 commit intents recorded
  against the file, the 7 symbols it contains, and `contains`/`refactored`/
  `modified`/`calls` edges — but not a single other Rust file, because Rust
  ingestion emitted no inter-file `imports` edges at all. An agent reading that
  clean result would conclude nothing depended on the file, which the graph had
  never actually said. Meanwhile `docs/EVALUATION.md` reported 1.00 precision on
  the impact question, measured on a **JS/TS** fixture where those edges exist.
  Rust files now declare their neighbours: `mod x;` resolves to the declaring
  module's directory, and `use crate::`/`self::`/`super::` resolve through a
  longest-first candidate ladder that the store picks a known file from — the
  same mechanism the JavaScript arm already used, because a `use` path cannot be
  split into module part and item part by looking at it
  (`crate::graph::GraphStore` and `crate::graph::query` are the same shape).
  What still does not resolve, deliberately:
  1. **Another workspace crate is a package, not a file.** `use
     mindleak_storage::resolve_database` records `package:mindleak_storage`
     rather than guessing `crates/mindleak-storage/src/lib.rs`, because the
     crate-name-to-directory mapping is a convention this code cannot verify.
     Cross-crate impact therefore stops at the crate boundary.
  2. **Nested use-groups are read one level deep.** `use a::{b::{c}, d}` binds
     the outer leaves; the inner group is not recursed. Rare in this codebase
     and it under-reports rather than inventing an edge.
  Both under-report, which is the safe direction: a missing edge is a smaller
  lie than a fabricated one.
  While confirming the original measurement: `AGENTS.md` had excluded
  `get_impact_radius` from the checklist on the grounds that it, like `recall`,
  "returns plausible strangers", citing this section — which only ever
  substantiated the `recall`
  half. The two were conflated: `recall` answers by embedding similarity and
  genuinely can return a stranger, whereas the impact radius is a deterministic
  traversal over recorded edges. Corrected in ADR-0066.

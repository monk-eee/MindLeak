- **Goal bindings did not cover the code that serves the goal — MEASURED,
  FIXED.** 47 of 131 source files under `crates/*/src` were bound to no goal,
  including the whole of `ingest/**` (the zero-token write path) and the whole
  post-split `facade/conformance/**`; two bindings still named
  `facade/conformance.rs` and `store/design.rs`, deleted by the module splits.
  — Medium impact: conformance cannot tell drift from an unbound file, so honest
  changes and real drift both come back silent. — **Fixed Jul 2026:** all files
  bound to their owning goal, dead bindings pruned, and
  `scripts/binding-audit.mjs --check` added so it cannot regress unnoticed
  (Lodestar task `task:7c3a63f1cfd3`).

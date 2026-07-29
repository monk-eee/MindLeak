- **`make reingest` lets an extractor improvement reach the graph that already
  exists.** Structural extraction happens once, at ingest time, and nothing
  revisited it: `reconcile_workspace` only forgets files that vanished, `index`
  only fills embeddings, and the editor sensor re-ingests a file only when
  somebody saves it. So when the extractor learned Rust `mod`/`use` edges, the
  3,703 artifact nodes already in the graph did not learn anything — each would
  have caught up only on its next save, silently, over months.
  Measured 2026-07-29, immediately after Rust import extraction shipped:
  `get_impact_radius` on `crates/mindleak-core/src/model.rs`, which nearly every
  module in the crate imports, returned 11 nodes, 11 edges and **zero** imports
  edges. The improvement was real and completely invisible. After one pass:

  | `model.rs` impact | before | after |
  |---|--:|--:|
  | nodes | 11 | 189 |
  | edges | 11 | 216 |
  | `imports` edges | 0 | 41 |
  | dependent `.rs` files reached | 0 | 25 |

  The pass enumerates tracked files with `git ls-files`, skips what the
  extractor cannot read, and drives `ingest_file` through a server it builds and
  spawns itself — deliberately not whichever server an editor is running, since
  a rebuilt binary does not change an already-running process, and that stale
  process is exactly what the pass exists to get past. Re-ingesting is safe by
  construction: `replace_structure` atomically replaces everything an artifact
  emitted.
  The cost is stated rather than hidden: re-asserting a structural edge resets
  its decay clock, so the structural tier reads as uniformly fresh afterwards.
  That is defensible for structure, which is true exactly as long as the file
  says so, and attention (`observed`) edges are not written by this pass.
  The first run also surfaced that 43 of 247 tracked files cannot be re-ingested
  at all, because an absolute id from a sibling worktree owns their structural
  edges. Recorded in the Known gaps of `DEVELOPERS.md`; it is an ownership
  decision rather than a patch.

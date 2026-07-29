- **An extractor improvement does not reach the graph that already exists —
  MITIGATED by `make reingest`, still not automatic.** Structural extraction
  happens once, at ingest time, and nothing revisits it: `reconcile_workspace`
  only forgets files that vanished, `index` only fills embeddings, and the
  editor sensor re-ingests a file only when someone saves it. Measured
  2026-07-29, immediately after Rust `mod`/`use` extraction shipped:
  `get_impact_radius` on `crates/mindleak-core/src/model.rs` — imported by
  nearly every module in the crate — returned 11 nodes, 11 edges and **zero**
  imports edges. The improvement was real and entirely invisible. After one
  `make reingest` pass the same query returned **189 nodes, 216 edges, 41
  imports edges and 25 dependent `.rs` files**.
  Two things remain. Nobody is told the graph is stale: no node records which
  extractor version produced it, so "this file was last understood three
  releases ago" is not a question the graph can answer, and the only symptom is
  an impact result that is quietly too small. And re-ingesting resets the decay
  clock on the structural edges it re-asserts — defensible, because structure is
  true as long as the file says so, but it means the structural tier reads as
  uniformly fresh afterwards. Attention (`observed`) edges are untouched.

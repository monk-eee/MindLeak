- **A conformance gate that checked nothing no longer reports OK.**
  `scripts/conformance-gate.mjs` printed `OK — N changed path(s), no governed
  gaps` whether it had verified every governed change or had inspected nothing
  at all. Those are not the same result, and on this repository it is nearly
  always the second: measured 2026-07-29, the constitution binds **8 code nodes
  and none of them are under `crates/`**, so a pull request touching fifty Rust
  files passed the gate having checked none of them — and said so in the words
  of a pass.
  That is the same shape as a conformance receipt that is `aligned` over an
  empty bundle, which this repository has already corrected once: agreement
  about nothing is not proof. The gate now returns what it was able to inspect
  (`inScope`, `ungoverned`, `governedNodes`) and reports `CHECKED NOTHING —
  none of N changed code path(s) are governed` when no changed path was in
  scope, naming how few nodes the constitution binds. Documentation is excluded
  from the ungoverned count, so a docs-only change does not read as a gap
  governance never claimed.
  Reporting only. Nothing new fails, and the dangling-binding check is
  unchanged. Two larger findings are recorded in the Known gaps of
  `DEVELOPERS.md` rather than acted on: 127 of 131 receipts cover zero governed
  nodes, and the gate cannot currently run in CI at all, because it reads an
  exported manifest that `.gitignore` excludes by policy.

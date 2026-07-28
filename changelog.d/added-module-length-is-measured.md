- **Module length is now measured by a committed script, so the number the
  constitution ratchets against can be reproduced by anyone.**
  `scripts/measure-module-length.mjs` counts Rust source modules under
  `crates/` whose non-test length exceeds 450 lines — measuring above the
  colocated test block, so a well-tested module is not mistaken for a bloated
  one, and excluding integration suites and `tests.rs` modules outright,
  because splitting those pays nothing. The count is deliberately an advisory
  signal rather than a verdict: the governing clause says file length is
  "resolved by human judgment", and a genuinely cohesive module may sit above
  the line. What the bound ratchet prevents is the count drifting upward
  unnoticed — a module crossing the threshold surfaces at review, where it is
  either split or a new baseline is accepted, and an accepted baseline is
  attributed and version-bumped, which is how the cohesion exception ends up
  stated and justified instead of forgotten.

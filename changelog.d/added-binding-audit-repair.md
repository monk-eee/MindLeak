- `binding-audit --repair` now applies the one binding repair that needs no
  judgement. When a bound module is split — `X.rs` becoming `X/mod.rs` plus
  siblings, which the `rust-module-length` control actively asks for — the
  binding is moved onto the descendants with its goal and mode unchanged, and
  the dead path retired. Four occurrences of that shape are recorded in
  `gaps.d`, each fixed identically by hand; the repair was always mechanical and
  is now mechanised. Writes go through `constitution_define`, never SQL, and the
  plan prints on every run so `--repair` applies exactly what a plain run just
  showed. A file split locally but still whole on another branch has its
  descendants bound *without* retiring the old path, because unbinding it would
  strip governance from unmerged work. A genuine deletion is still reported and
  left alone.

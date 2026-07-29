- **A Known gap that reported 8 governed nodes now reports 161, and says when
  each figure was taken.** The entry claiming the conformance chain governed 8
  code nodes — none of them Rust — with 127 of 131 receipts covering nothing was
  measured at 03:37Z and was already wrong by 09:29Z: 161 governed nodes, 133 of
  them `.rs` under `crates/`, and 72 of 172 receipts covering zero nodes. The
  gap was real and another agent closed it within the day.
  Corrected rather than deleted, because the shape of the mistake is the useful
  part: the number was right when taken and stale within hours, and a Known gap
  that records a measurement without its timestamp keeps being read as current
  long after it stops being. Both measurements are now shown side by side with
  their times.
  It also names `scripts/binding-audit.mjs` as the way to re-measure, which
  already existed and which the original entry did not mention — leaving the
  next reader to rebuild an audit that was sitting in `scripts/`. As of 09:29Z
  it reports 131 of 136 source files bound and names the five that are not,
  every one of them recently added. That is the residual gap worth watching: a
  binding is applied to the tree as it was, so a new module arrives ungoverned
  and nothing says so.
  The half of the entry that is still true is re-verified and kept:
  `conformance-gate.mjs` still cannot run, because `.gitignore` still excludes
  the manifest it reads and nothing is tracked under `.lodestar/`.

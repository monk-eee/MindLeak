- **`tasks.claim_lapses` and `tasks.unleased_seconds` are gone; continuity is
  derived from the log.** ADR-0064 decision 5. The two running totals the claim
  compare-and-swap maintained are replaced by `claim_window`, which replays the
  recorded transitions. Their agreement was proved in the preceding commit while
  both still existed; the migration drops the columns only *after* the genesis
  import has carried their values into the log, because they are the sole
  surviving trace of a window that opened before the log did.
  `ALTER TABLE ... DROP COLUMN`, never a table rebuild: a rebuild rewrites every
  row including `owner` on live claims, and ADR-0063 is explicit that a live
  claim is not ours to touch. Dropping an unrelated column moves nothing.
  The fields are **not** kept on `Task` as derived values. Zero lapses means
  "this window may certify itself as aligned", so a field any read path could
  leave unpopulated would fail *open* — quietly handing out a clean receipt for
  work with holes in it. Conformance and the conformance token now ask for the
  window explicitly; there is no field to forget.
  Board rows carry `claim_window` instead, so the continuity a reader needs is
  still beside the status rather than a query away. `scripts/stranded-report.mjs`
  reads it from there.

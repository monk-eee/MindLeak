- **The migration engine and the one-shot repairs it runs are separate
  modules.** `db/migrations.rs` had grown to 476 non-test lines and failed the
  `rust-module-length` ratchet against
  `goal:source-files-stay-small-and-cohesive`, because the file held two things
  with different reasons to change: an engine that runs migrations, and the
  individual data repairs it runs. The repairs accumulate — every defect that
  reaches production adds one, each carrying the long explanation of what went
  wrong — while the engine does not grow. They now live in `db/repairs.rs`, and
  the engine keeps the shared `column_exists` helper and the `run_once` guard.
  No migration behaviour changes: the same repairs run in the same order under
  the same guard. The control passes again at 7 of 132 modules over the limit.

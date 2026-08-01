- **Binding coverage is silently dark when the audit reads a spec.db that
  predates the `goal_artifacts` rename — OBSERVED 2026-08-01, OPEN.** Every
  `canonical-push` across a long fleet session ended with `no such table:
  goal_artifacts` immediately followed by `canonical-push: binding coverage was
  not observed for this publication` — on four separate publications.

  The mechanism is a migration the reader never triggers.
  `scripts/binding-audit.mjs` opens the per-repository `spec.db` read-only via
  Node's `DatabaseSync` and runs `select goal_id, node_id, mode from
  goal_artifacts` (around line 155). Node's SQLite does not run the Rust
  migrations; `rename_goal_code_to_goal_artifacts`
  (`crates/lodestar-core/src/db/migrations.rs`) only runs when a lodestar-mcp
  binary new enough to carry it opens the DB. So a `spec.db` last migrated by a
  pre-rename binary still holds `goal_code`, the audit's query throws, and
  canonical-push swallows the throw into a soft "not observed".

  Impact: the control that flags a newly added Rust module no goal governs is
  off on every affected publication, and it fails soft rather than loud — the
  push succeeds and prints a benign-looking line, so nobody notices the guard is
  dark and an ungoverned module ships unnoticed. This is the same "a mechanism
  that exists, works, and runs nowhere" shape the repo has been burned by before.

  The cause is split: operationally the installed lodestar-mcp binary that
  migrates the DB is stale (rebuild + `node scripts/install-servers.mjs`), and
  structurally the audit reads a DB it never ensures is migrated. Fix direction
  (left for later, needs a decision): have `binding-audit.mjs` tolerate the
  pre-rename schema — read `goal_code`, with its possibly-absent `mode` column,
  when `goal_artifacts` is missing — or ensure the DB is migrated before the
  read; and make canonical-push treat a thrown audit as a visible failure, not a
  quiet "not observed". Recording the measurement this run, not fixing the
  control.

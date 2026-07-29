- **Duplicate `define_goal` title+statement surfaces a raw SQLite error.** — A
  third goal sharing a title and statement collides on the derived
  `goal:{slug}-{hash(statement)}` id and fails with an opaque `UNIQUE
  constraint` error instead of a typed `LodestarError::Invalid`. — Low impact
  (edge case; goals are rarely exact duplicates). — **Fixed Jul 2026:**
  `store::define_goal` pre-checks the derived id and returns a typed
  `LodestarError::Invalid` pointing the author at `supersede_goal`; regression
  test `redefining_an_identical_goal_is_a_typed_error_not_a_raw_sqlite_fault`.

- **Creating an identical task in the same second now returns a typed domain
  error instead of leaking SQLite.** Task ids are derived from goal id, title,
  and a whole-second timestamp. Two identical creates inside that second used
  to let the second `INSERT` hit the primary key and return `UNIQUE constraint
  failed: tasks.id` — an implementation detail for what is plainly a duplicate
  request. `create_task_after_on` now checks the derived id before dependency
  validation or insertion and returns `LodestarError::Invalid`, identifying the
  existing task and telling the caller to reuse it or choose a distinct title.
  The first task remains unchanged and no second row is written. A focused
  regression test was proven red against the previous implementation and green
  with the pre-check.

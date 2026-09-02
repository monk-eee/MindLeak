- The Industrial Work event stream now allocates a repository-scoped
  `stream_position` for every event, and the task projection records the
  `source_event_position` it was built from. Work was the only event-sourced
  domain in Ackplane without them: `work_task_history` ordered itself by
  `recorded_at`, a clock reading that ties and leaves gaps, so nothing could
  answer whether the projection had seen every event up to a given point.
  ADR-0120 decision 6's `lagging` publication state depends on exactly that
  comparison and was previously unstateable rather than merely unwritten.
  Existing history rows are backfilled in `recorded_at`/`event_id` order and
  each repository's stream head is seeded past them, so positions assigned
  after the migration cannot collide with a backfilled row.

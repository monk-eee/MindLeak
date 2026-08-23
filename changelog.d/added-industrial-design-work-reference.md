### Added

- `industrial_designs` (ADR-0121 decision 3) now enforces its `work_task_id`
  reference with a real foreign key into `work_tasks` (ADR-0120), added by a
  follow-up migration now that the Work domain's own schema has landed. The
  reference was deferred out of the original migration
  (`0027_industrial_designs.sql`) until `work_tasks` existed on `main`.

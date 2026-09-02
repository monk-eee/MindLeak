-- ADR-0120 decision 3: the Industrial Work domain is an append-only event
-- stream with a checked projection. The stream was built without the one thing
-- that makes it a stream rather than a bag of rows -- an allocated,
-- repository-scoped position. `work_task_history` ordered itself by
-- `recorded_at`, which is a clock reading: it ties, it is not dense, and it
-- cannot answer "has the projection seen every event up to here?".
--
-- Every other event-sourced domain in this schema already carries the shape
-- this migration gives Work: a `*_stream_heads` row holding the highest
-- position handed out per (tenant, repository), a `stream_position` on each
-- event, and a `source_event_position` on the projection recording the event
-- that produced its current state. See `0001_ledger.sql`,
-- `0022_human_delegation.sql`, and `0054_human_decision_requests.sql`.
--
-- Without it, ADR-0120 decision 6's `lagging` publication state -- "the
-- projection has fallen behind the ledger" -- is not merely unwritten but
-- unstateable, because there is no position on either side to compare.

CREATE TABLE IF NOT EXISTS work_stream_heads (
    tenant_id       TEXT   NOT NULL,
    repository_id   TEXT   NOT NULL,
    stream_position BIGINT NOT NULL CHECK (stream_position >= 0),
    PRIMARY KEY (tenant_id, repository_id)
);

ALTER TABLE work_task_history ADD COLUMN IF NOT EXISTS stream_position BIGINT;
ALTER TABLE work_tasks ADD COLUMN IF NOT EXISTS source_event_position BIGINT;

-- Backfill: the earlier attempt's rows (see the reconciliation note in
-- `0028_work.sql`) predate the column. `recorded_at` is the only ordering they
-- ever had, so it is the only honest basis for assigning them one; `event_id`
-- breaks a tie deterministically so the same database always backfills to the
-- same positions.
WITH ordered AS (
    SELECT tenant_id,
           repository_id,
           event_id,
           row_number() OVER (
               PARTITION BY tenant_id, repository_id
               ORDER BY recorded_at ASC, event_id ASC
           ) AS assigned
    FROM work_task_history
)
UPDATE work_task_history AS history
   SET stream_position = ordered.assigned
  FROM ordered
 WHERE history.tenant_id = ordered.tenant_id
   AND history.repository_id = ordered.repository_id
   AND history.event_id = ordered.event_id
   AND history.stream_position IS NULL;

ALTER TABLE work_task_history ALTER COLUMN stream_position SET NOT NULL;

ALTER TABLE work_task_history DROP CONSTRAINT IF EXISTS work_task_history_stream_position_check;
ALTER TABLE work_task_history ADD CONSTRAINT work_task_history_stream_position_check
    CHECK (stream_position > 0);

-- One event per position per repository: this is what makes a gap in the
-- sequence mean "an event is missing" rather than "two events shared a slot".
CREATE UNIQUE INDEX IF NOT EXISTS work_task_history_by_stream_position
    ON work_task_history (tenant_id, repository_id, stream_position);

-- The projection points at the event that produced its current state. It stays
-- nullable on purpose, with the same meaning `0002_projection.sql` gives it:
-- NULL is "never projected", which is a different fact from position zero.
ALTER TABLE work_tasks DROP CONSTRAINT IF EXISTS work_tasks_source_event_position_check;
ALTER TABLE work_tasks ADD CONSTRAINT work_tasks_source_event_position_check
    CHECK (source_event_position IS NULL OR source_event_position > 0);

UPDATE work_tasks AS task
   SET source_event_position = latest.stream_position
  FROM (
       SELECT tenant_id,
              repository_id,
              task_id,
              MAX(stream_position) AS stream_position
         FROM work_task_history
        GROUP BY tenant_id, repository_id, task_id
       ) AS latest
 WHERE task.tenant_id = latest.tenant_id
   AND task.repository_id = latest.repository_id
   AND task.task_id = latest.task_id
   AND task.source_event_position IS NULL;

-- Seed each head past the positions the backfill just handed out, so the first
-- allocation after this migration cannot collide with a backfilled row.
INSERT INTO work_stream_heads (tenant_id, repository_id, stream_position)
SELECT tenant_id, repository_id, MAX(stream_position)
  FROM work_task_history
 GROUP BY tenant_id, repository_id
    ON CONFLICT (tenant_id, repository_id)
    DO UPDATE SET stream_position =
        GREATEST(work_stream_heads.stream_position, EXCLUDED.stream_position);

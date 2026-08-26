-- ADR-0125 slice 3: task-level optimistic concurrency (closing ADR-0120
-- decision 3's deliberately deferred "expected-prior-version" scheme) plus
-- the event kinds the server-owned command effects append. AnswerWait
-- mutates work_task_waits directly and needs no new event kind.
ALTER TABLE work_tasks ADD COLUMN IF NOT EXISTS version BIGINT NOT NULL DEFAULT 1;
ALTER TABLE work_tasks ADD COLUMN IF NOT EXISTS route_reference TEXT
    CHECK (route_reference IS NULL OR octet_length(route_reference) BETWEEN 1 AND 256);

ALTER TABLE work_task_history DROP CONSTRAINT IF EXISTS work_task_history_event_kind_check;
ALTER TABLE work_task_history ADD CONSTRAINT work_task_history_event_kind_check
    CHECK (event_kind BETWEEN 1 AND 5);

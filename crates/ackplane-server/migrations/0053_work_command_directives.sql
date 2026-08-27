-- ADR-0125 slice 4: Assign/Steer/Pause/Resume/Drain request a matching
-- ADR-0107 directive rather than mutating work_tasks immediately (decision
-- 7). `directive_id` links a command back to the directive its confirm step
-- issued, so the directive's own later applied/refused/failed/expired
-- receipt can be traced back to the command it must append a Work event and
-- receipt for. It stays NULL for the five server-owned command kinds, which
-- never issue a directive.
ALTER TABLE work_commands ADD COLUMN IF NOT EXISTS directive_id TEXT
    CHECK (directive_id IS NULL OR octet_length(directive_id) BETWEEN 1 AND 256);

CREATE UNIQUE INDEX IF NOT EXISTS work_commands_by_directive
    ON work_commands (tenant_id, repository_id, directive_id)
    WHERE directive_id IS NOT NULL;

-- New work_task_history event kinds for the five supervisor-directed
-- commands, numbered 6-10 to match WorkCommandKind::Assign..Drain (1-5 are
-- CreateWork/RouteWork/ReleaseLease/SubmitReview/AnswerWait, already in use).
ALTER TABLE work_task_history DROP CONSTRAINT IF EXISTS work_task_history_event_kind_check;
ALTER TABLE work_task_history ADD CONSTRAINT work_task_history_event_kind_check
    CHECK (event_kind BETWEEN 1 AND 10);

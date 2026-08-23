-- ADR-0121 decision 3's Work reference, deferred in 0027 until the Work
-- domain's own schema existed (gaps.d/industrial-design-has-no-work-task-
-- reference-yet.md). Additive: expands the existing table, never rewrites
-- migration 0027's own DDL.
ALTER TABLE industrial_designs
    ADD COLUMN IF NOT EXISTS work_task_id TEXT;

ALTER TABLE industrial_designs
    ADD CONSTRAINT industrial_designs_work_task_fkey
    FOREIGN KEY (tenant_id, repository_id, work_task_id)
    REFERENCES work_tasks (tenant_id, repository_id, task_id);

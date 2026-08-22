-- Supports bounded Evidence Board review-state filters while preserving the
-- existing server-recorded keyset order within one tenant, repository, task,
-- and selected review state.
CREATE INDEX IF NOT EXISTS conformance_records_by_task_review_recorded
    ON conformance_records (
        tenant_id,
        repository_id,
        task_id,
        review_state,
        recorded_at DESC,
        conformance_id ASC
    );

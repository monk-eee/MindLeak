-- Indexes, applied AFTER migrations (db.rs).
--
-- They live apart from schema.sql because an index over a column that a
-- migration adds cannot be created before that migration runs. On an existing
-- database CREATE TABLE IF NOT EXISTS is a no-op, so the old table is still in
-- place when the index statement executes; the whole batch then fails and the
-- migrations that would have fixed it never run. That is not hypothetical:
-- idx_task_qa_audience did exactly this and made every pre-existing database
-- unopenable by a current build.
--
-- Keeping every index here makes the ordering structural rather than something
-- each new migration has to remember.

CREATE INDEX IF NOT EXISTS idx_constitution_status ON constitution_versions(status);
CREATE INDEX IF NOT EXISTS idx_controls_clause ON controls(clause_id);
CREATE INDEX IF NOT EXISTS idx_waivers_clause ON waivers(clause_id);
CREATE INDEX IF NOT EXISTS idx_waivers_expiry ON waivers(expires_at);
CREATE INDEX IF NOT EXISTS idx_amendments_to ON constitution_amendments(to_version);
CREATE INDEX IF NOT EXISTS idx_goals_status ON goals(status);
CREATE INDEX IF NOT EXISTS idx_goals_slug   ON goals(slug);
CREATE INDEX IF NOT EXISTS idx_design_items_status ON design_items(status);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_goal   ON tasks(goal_id);
CREATE INDEX IF NOT EXISTS idx_tasks_blocked_by ON tasks(blocked_by);
CREATE INDEX IF NOT EXISTS idx_task_claim_transfers_task
    ON task_claim_transfers(task_id, id);
-- The task log is read per task to derive evidence-window continuity
-- (ADR-0064 d5), so every claim_window call is this lookup.
CREATE INDEX IF NOT EXISTS idx_task_events_task ON task_events(task_id, seq);
CREATE INDEX IF NOT EXISTS idx_task_scopes_value ON task_scopes(kind, value);
CREATE INDEX IF NOT EXISTS idx_task_qa_task ON task_qa(task_id);
CREATE INDEX IF NOT EXISTS idx_task_qa_audience ON task_qa(audience, kind);
CREATE INDEX IF NOT EXISTS idx_task_goal_coverage_goal ON task_goal_coverage(goal_id);
CREATE INDEX IF NOT EXISTS idx_goal_code_node ON goal_code(node_id);
CREATE INDEX IF NOT EXISTS idx_session_context_base ON session_context(base);

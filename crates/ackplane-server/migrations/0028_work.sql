-- ADR-0120: an Industrial repository has one optional Ackplane-authoritative
-- Work namespace -- a task projection plus a bounded event/history log, a
-- typed-wait log, and a checkpoint log. `work_tasks` is the current-state
-- projection; the other three are append-only records referencing it.
--
-- RECONCILIATION NOTE: an earlier, unaccepted attempt at this domain already
-- created these four tables directly against the shared development
-- database before ADR-0120 was accepted (gaps.d/unaccepted-work-migration-
-- reaches-shared-db.md), and `industrial_designs` (a concurrent, separate
-- feature) has since taken a foreign key against `work_tasks`. This
-- migration is therefore written to match that already-live shape via
-- `CREATE TABLE IF NOT EXISTS`, and widens `work_tasks_state_check` from the
-- earlier attempt's 7-state range to the 8 states ADR-0120 decision 3 names
-- (`open, claimed, waiting, paused, blocked, in_review, completed,
-- abandoned`), which the earlier draft schema had not yet finalized.
CREATE TABLE IF NOT EXISTS work_tasks (
    tenant_id          TEXT        NOT NULL,
    repository_id      TEXT        NOT NULL,
    task_id            TEXT        NOT NULL,
    title              TEXT        NOT NULL CHECK (octet_length(title) BETWEEN 1 AND 512),
    acceptance         TEXT        NOT NULL CHECK (octet_length(acceptance) BETWEEN 1 AND 16384),
    goal_id            TEXT        CHECK (goal_id IS NULL OR octet_length(goal_id) BETWEEN 1 AND 256),
    state              SMALLINT    NOT NULL CHECK (state BETWEEN 1 AND 7),
    owner_id           TEXT        CHECK (owner_id IS NULL OR octet_length(owner_id) BETWEEN 1 AND 256),
    owner_session_id   TEXT        CHECK (owner_session_id IS NULL OR octet_length(owner_session_id) BETWEEN 1 AND 256),
    lease_expires_at   TIMESTAMPTZ CHECK (lease_expires_at IS NULL OR owner_id IS NOT NULL),
    declared_paths     TEXT[]      NOT NULL DEFAULT '{}',
    declared_symbols   TEXT[]      NOT NULL DEFAULT '{}',
    source_digest      BYTEA       NOT NULL CHECK (octet_length(source_digest) = 32),
    published_by       TEXT        NOT NULL CHECK (octet_length(published_by) BETWEEN 1 AND 256),
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, task_id)
);

CREATE INDEX IF NOT EXISTS work_tasks_by_tenant_state_updated
    ON work_tasks (tenant_id, repository_id, state, updated_at DESC, task_id);

ALTER TABLE work_tasks DROP CONSTRAINT IF EXISTS work_tasks_state_check;
ALTER TABLE work_tasks ADD CONSTRAINT work_tasks_state_check CHECK (state BETWEEN 1 AND 8);

CREATE TABLE IF NOT EXISTS work_task_history (
    tenant_id       TEXT        NOT NULL,
    repository_id   TEXT        NOT NULL,
    event_id        TEXT        NOT NULL,
    task_id         TEXT        NOT NULL,
    event_kind      SMALLINT    NOT NULL CHECK (event_kind BETWEEN 1 AND 4),
    from_state      SMALLINT    CHECK (from_state IS NULL OR from_state BETWEEN 1 AND 8),
    to_state        SMALLINT    NOT NULL CHECK (to_state BETWEEN 1 AND 8),
    actor_id        TEXT        NOT NULL CHECK (octet_length(actor_id) BETWEEN 1 AND 256),
    source_digest   BYTEA       NOT NULL CHECK (octet_length(source_digest) = 32),
    recorded_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, event_id),
    FOREIGN KEY (tenant_id, repository_id, task_id)
        REFERENCES work_tasks (tenant_id, repository_id, task_id) ON DELETE CASCADE
);

ALTER TABLE work_task_history DROP CONSTRAINT IF EXISTS work_task_history_from_state_check;
ALTER TABLE work_task_history ADD CONSTRAINT work_task_history_from_state_check
    CHECK (from_state IS NULL OR from_state BETWEEN 1 AND 8);
ALTER TABLE work_task_history DROP CONSTRAINT IF EXISTS work_task_history_to_state_check;
ALTER TABLE work_task_history ADD CONSTRAINT work_task_history_to_state_check
    CHECK (to_state BETWEEN 1 AND 8);

CREATE INDEX IF NOT EXISTS work_task_history_by_task_recorded
    ON work_task_history (tenant_id, repository_id, task_id, recorded_at DESC, event_id);

CREATE TABLE IF NOT EXISTS work_task_waits (
    tenant_id     TEXT        NOT NULL,
    repository_id TEXT        NOT NULL,
    wait_id       TEXT        NOT NULL,
    task_id       TEXT        NOT NULL,
    question      TEXT        NOT NULL CHECK (octet_length(question) BETWEEN 1 AND 16384),
    audience      TEXT        CHECK (audience IS NULL OR octet_length(audience) BETWEEN 1 AND 256),
    asked_by      TEXT        NOT NULL CHECK (octet_length(asked_by) BETWEEN 1 AND 256),
    asked_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    answered_by   TEXT        CHECK (answered_by IS NULL OR octet_length(answered_by) BETWEEN 1 AND 256),
    answer        TEXT,
    answered_at   TIMESTAMPTZ,
    PRIMARY KEY (tenant_id, repository_id, wait_id),
    CHECK (
        (answer IS NULL AND answered_at IS NULL AND answered_by IS NULL)
        OR (answer IS NOT NULL AND answered_at IS NOT NULL AND answered_by IS NOT NULL)
    ),
    FOREIGN KEY (tenant_id, repository_id, task_id)
        REFERENCES work_tasks (tenant_id, repository_id, task_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS work_task_checkpoints (
    tenant_id      TEXT        NOT NULL,
    repository_id  TEXT        NOT NULL,
    checkpoint_id  TEXT        NOT NULL,
    task_id        TEXT        NOT NULL,
    state          SMALLINT    NOT NULL CHECK (state BETWEEN 1 AND 7),
    summary        TEXT        NOT NULL CHECK (octet_length(summary) BETWEEN 1 AND 16384),
    recorded_by    TEXT        NOT NULL CHECK (octet_length(recorded_by) BETWEEN 1 AND 256),
    source_digest  BYTEA       NOT NULL CHECK (octet_length(source_digest) = 32),
    recorded_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, checkpoint_id),
    FOREIGN KEY (tenant_id, repository_id, task_id)
        REFERENCES work_tasks (tenant_id, repository_id, task_id) ON DELETE CASCADE
);

ALTER TABLE work_task_checkpoints DROP CONSTRAINT IF EXISTS work_task_checkpoints_state_check;
ALTER TABLE work_task_checkpoints ADD CONSTRAINT work_task_checkpoints_state_check
    CHECK (state BETWEEN 1 AND 8);

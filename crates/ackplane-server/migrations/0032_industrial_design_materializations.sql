-- ADR-0121 decision 4: materialization is revisioned and referential, not a
-- task-generation side effect. A revision's identity fields never mutate;
-- an identical resubmission (same idempotency_key + content) is a no-op, a
-- changed one under the same idempotency_key is refused by the store layer
-- (matching evidence_records' own established idempotency-key contract).
CREATE TABLE IF NOT EXISTS industrial_design_materializations (
    tenant_id               TEXT        NOT NULL,
    repository_id           TEXT        NOT NULL,
    design_id               TEXT        NOT NULL,
    revision_number         BIGINT      NOT NULL,
    actor                   TEXT        NOT NULL,
    idempotency_key         TEXT        NOT NULL,
    rationale               TEXT,
    constitution_version_id TEXT        NOT NULL,
    goal_ids                TEXT[]      NOT NULL DEFAULT '{}',
    payload_digest          BYTEA       NOT NULL,
    recorded_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, design_id, revision_number),
    CONSTRAINT industrial_design_materializations_actor_check
        CHECK (octet_length(actor) >= 1 AND octet_length(actor) <= 256),
    CONSTRAINT industrial_design_materializations_idempotency_key_check
        CHECK (octet_length(idempotency_key) >= 1 AND octet_length(idempotency_key) <= 256),
    CONSTRAINT industrial_design_materializations_rationale_check
        CHECK (rationale IS NULL OR octet_length(rationale) <= 8192),
    CONSTRAINT industrial_design_materializations_goal_ids_check
        CHECK (array_length(goal_ids, 1) IS NULL OR array_length(goal_ids, 1) <= 32),
    CONSTRAINT industrial_design_materializations_digest_check
        CHECK (octet_length(payload_digest) = 32),
    CONSTRAINT industrial_design_materializations_design_fkey
        FOREIGN KEY (tenant_id, repository_id, design_id)
        REFERENCES industrial_designs (tenant_id, repository_id, design_id),
    CONSTRAINT industrial_design_materializations_constitution_fkey
        FOREIGN KEY (tenant_id, repository_id, constitution_version_id)
        REFERENCES constitution_publications (tenant_id, repository_id, version_id)
);

CREATE UNIQUE INDEX IF NOT EXISTS industrial_design_materializations_idempotency_idx
    ON industrial_design_materializations (tenant_id, repository_id, design_id, idempotency_key);

-- Bounded, FK-checked references from one materialization revision to the
-- Industrial Work tasks it resulted from or produced. A junction table
-- rather than an array column so each reference is a real foreign key,
-- matching every other cross-domain reference in this crate.
CREATE TABLE IF NOT EXISTS industrial_design_materialization_work_tasks (
    tenant_id       TEXT   NOT NULL,
    repository_id   TEXT   NOT NULL,
    design_id       TEXT   NOT NULL,
    revision_number BIGINT NOT NULL,
    work_task_id    TEXT   NOT NULL,
    PRIMARY KEY (tenant_id, repository_id, design_id, revision_number, work_task_id),
    CONSTRAINT industrial_design_materialization_work_tasks_revision_fkey
        FOREIGN KEY (tenant_id, repository_id, design_id, revision_number)
        REFERENCES industrial_design_materializations (tenant_id, repository_id, design_id, revision_number),
    CONSTRAINT industrial_design_materialization_work_tasks_task_fkey
        FOREIGN KEY (tenant_id, repository_id, work_task_id)
        REFERENCES work_tasks (tenant_id, repository_id, task_id)
);

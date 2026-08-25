-- ADR-0125: an authoritative command service records immutable Work command
-- requests and receipts after it authorizes a caller, before delivery or any
-- Work/Claim mutation occurs.
CREATE TABLE IF NOT EXISTS work_commands (
    tenant_id              TEXT        NOT NULL,
    repository_id          TEXT        NOT NULL,
    command_id             TEXT        NOT NULL,
    command_kind           SMALLINT    NOT NULL CHECK (command_kind BETWEEN 1 AND 10),
    schema_version         TEXT        NOT NULL,
    task_id                TEXT,
    issuing_principal_id   TEXT        NOT NULL,
    delegation_id          TEXT,
    policy_refs            TEXT[]      NOT NULL DEFAULT '{}',
    rationale              TEXT        NOT NULL,
    expected_task_version  BIGINT      CHECK (expected_task_version IS NULL OR expected_task_version >= 0),
    confirmation_id        TEXT,
    expires_at             TIMESTAMPTZ NOT NULL,
    idempotency_key        TEXT        NOT NULL,
    request_digest         BYTEA       NOT NULL CHECK (octet_length(request_digest) = 32),
    payload_digest         BYTEA       NOT NULL CHECK (octet_length(payload_digest) = 32),
    recorded_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, command_id),
    UNIQUE (tenant_id, repository_id, issuing_principal_id, idempotency_key),
    FOREIGN KEY (tenant_id, repository_id, task_id)
        REFERENCES work_tasks (tenant_id, repository_id, task_id) ON DELETE RESTRICT,
    CHECK (octet_length(command_id) BETWEEN 1 AND 256),
    CHECK (octet_length(schema_version) BETWEEN 1 AND 256),
    CHECK (task_id IS NULL OR octet_length(task_id) BETWEEN 1 AND 256),
    CHECK (octet_length(issuing_principal_id) BETWEEN 1 AND 256),
    CHECK (delegation_id IS NULL OR octet_length(delegation_id) BETWEEN 1 AND 256),
    CHECK (octet_length(rationale) BETWEEN 1 AND 4096),
    CHECK (confirmation_id IS NULL OR octet_length(confirmation_id) BETWEEN 1 AND 256),
    CHECK (octet_length(idempotency_key) BETWEEN 1 AND 256)
);

CREATE TABLE IF NOT EXISTS work_command_receipts (
    tenant_id          TEXT        NOT NULL,
    repository_id      TEXT        NOT NULL,
    command_id         TEXT        NOT NULL,
    receipt_id         TEXT        NOT NULL,
    outcome            SMALLINT    NOT NULL CHECK (outcome BETWEEN 1 AND 8),
    reason             TEXT        NOT NULL DEFAULT '',
    evidence_refs      TEXT[]      NOT NULL DEFAULT '{}',
    receipt_digest     BYTEA       NOT NULL CHECK (octet_length(receipt_digest) = 32),
    occurred_at        TIMESTAMPTZ NOT NULL,
    recorded_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, receipt_id),
    UNIQUE (tenant_id, repository_id, command_id, receipt_digest),
    FOREIGN KEY (tenant_id, repository_id, command_id)
        REFERENCES work_commands (tenant_id, repository_id, command_id) ON DELETE CASCADE,
    CHECK (octet_length(receipt_id) BETWEEN 1 AND 256),
    CHECK (octet_length(reason) <= 4096)
);

CREATE INDEX IF NOT EXISTS work_commands_by_task_recorded
    ON work_commands (tenant_id, repository_id, task_id, recorded_at DESC, command_id);

CREATE INDEX IF NOT EXISTS work_command_receipts_by_command_recorded
    ON work_command_receipts (tenant_id, repository_id, command_id, recorded_at ASC, receipt_id);

-- ADR-0115: a delegation is not authority until each routine use is checked
-- against its live scope, policy basis, expiry, and budget. The receipt keeps
-- that decision immutable without retaining unbounded command or prompt data.
CREATE TABLE IF NOT EXISTS delegation_use_receipts (
    tenant_id                  TEXT        NOT NULL,
    repository_id              TEXT        NOT NULL,
    delegation_id              TEXT        NOT NULL,
    receipt_id                 BIGSERIAL   NOT NULL,
    issuer_principal_id        TEXT        NOT NULL,
    delegatee_session_id       TEXT        NOT NULL,
    project_id                 TEXT,
    task_id                    TEXT,
    goal_id                    TEXT        NOT NULL,
    policy_version             TEXT        NOT NULL,
    constitution_version       TEXT        NOT NULL,
    delegated_action           SMALLINT    NOT NULL CHECK (delegated_action BETWEEN 1 AND 7),
    reserved_token_budget      BIGINT      NOT NULL CHECK (reserved_token_budget >= 0),
    delegation_version         INTEGER     NOT NULL CHECK (delegation_version > 0),
    outcome                    SMALLINT    NOT NULL CHECK (outcome BETWEEN 1 AND 2),
    refusal_reason             SMALLINT,
    idempotency_key            TEXT        NOT NULL,
    payload_digest             BYTEA       NOT NULL CHECK (octet_length(payload_digest) = 32),
    recorded_at                TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, delegation_id, receipt_id),
    UNIQUE (tenant_id, repository_id, delegation_id, idempotency_key),
    FOREIGN KEY (tenant_id, repository_id, delegation_id)
        REFERENCES delegation_projections (tenant_id, repository_id, delegation_id)
        ON DELETE RESTRICT,
    CHECK (
        (outcome = 1 AND refusal_reason IS NULL)
        OR (outcome = 2 AND refusal_reason BETWEEN 1 AND 10)
    ),
    CHECK (octet_length(issuer_principal_id) BETWEEN 1 AND 256),
    CHECK (octet_length(delegatee_session_id) BETWEEN 1 AND 256),
    CHECK (project_id IS NULL OR octet_length(project_id) BETWEEN 1 AND 256),
    CHECK (task_id IS NULL OR octet_length(task_id) BETWEEN 1 AND 256),
    CHECK (octet_length(goal_id) BETWEEN 1 AND 256),
    CHECK (octet_length(policy_version) BETWEEN 1 AND 256),
    CHECK (octet_length(constitution_version) BETWEEN 1 AND 256),
    CHECK (octet_length(idempotency_key) BETWEEN 1 AND 256)
);

CREATE INDEX IF NOT EXISTS delegation_use_receipts_by_delegation
    ON delegation_use_receipts (
        tenant_id,
        repository_id,
        delegation_id,
        receipt_id ASC
    );

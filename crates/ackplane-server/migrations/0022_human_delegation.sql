-- ADR-0115: human-approved delegation is a bounded, tenant/repository-scoped
-- authority record. This is not a browser control plane or a Local task copy.
CREATE TABLE IF NOT EXISTS delegation_stream_heads (
    tenant_id       TEXT        NOT NULL,
    repository_id   TEXT        NOT NULL,
    stream_position BIGINT      NOT NULL CHECK (stream_position >= 0),
    PRIMARY KEY (tenant_id, repository_id)
);

CREATE TABLE IF NOT EXISTS delegation_events (
    tenant_id              TEXT        NOT NULL,
    repository_id          TEXT        NOT NULL,
    stream_position        BIGINT      NOT NULL CHECK (stream_position > 0),
    delegation_id          TEXT        NOT NULL,
    event_kind             SMALLINT    NOT NULL CHECK (event_kind BETWEEN 1 AND 2),
    actor_principal_id     TEXT        NOT NULL,
    expected_prior_version INTEGER     NOT NULL CHECK (expected_prior_version >= 0),
    resulting_version      INTEGER     NOT NULL CHECK (resulting_version > 0),
    idempotency_key        TEXT        NOT NULL,
    payload_digest         BYTEA       NOT NULL CHECK (octet_length(payload_digest) = 32),
    schema_version         SMALLINT    NOT NULL CHECK (schema_version = 1),
    recorded_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, stream_position),
    UNIQUE (tenant_id, repository_id, idempotency_key),
    CHECK (octet_length(delegation_id) BETWEEN 1 AND 256),
    CHECK (octet_length(actor_principal_id) BETWEEN 1 AND 256),
    CHECK (octet_length(idempotency_key) BETWEEN 1 AND 256)
);

CREATE TABLE IF NOT EXISTS delegation_projections (
    tenant_id                    TEXT        NOT NULL,
    repository_id                TEXT        NOT NULL,
    delegation_id                TEXT        NOT NULL,
    issuer_principal_id          TEXT        NOT NULL,
    delegatee_session_id         TEXT        NOT NULL,
    project_id                   TEXT,
    task_id                      TEXT,
    goal_id                      TEXT        NOT NULL,
    goal_digest                  BYTEA       NOT NULL CHECK (octet_length(goal_digest) = 32),
    policy_version               TEXT        NOT NULL,
    policy_digest                BYTEA       NOT NULL CHECK (octet_length(policy_digest) = 32),
    constitution_version         TEXT        NOT NULL,
    constitution_digest          BYTEA       NOT NULL CHECK (octet_length(constitution_digest) = 32),
    allowed_actions              SMALLINT[]  NOT NULL CHECK (array_length(allowed_actions, 1) BETWEEN 1 AND 16),
    max_token_budget             BIGINT      NOT NULL CHECK (max_token_budget > 0),
    max_actions_per_session      BIGINT      NOT NULL CHECK (max_actions_per_session > 0),
    source_protocol_version      SMALLINT    NOT NULL CHECK (source_protocol_version > 0),
    issued_at                    TIMESTAMPTZ NOT NULL,
    effective_at                 TIMESTAMPTZ NOT NULL,
    expires_at                   TIMESTAMPTZ NOT NULL,
    status                       SMALLINT    NOT NULL CHECK (status BETWEEN 1 AND 2),
    version                      INTEGER     NOT NULL CHECK (version > 0),
    source_event_position        BIGINT      NOT NULL CHECK (source_event_position > 0),
    revoked_at                   TIMESTAMPTZ,
    revoked_by_principal_id      TEXT,
    revocation_reason            TEXT,
    PRIMARY KEY (tenant_id, repository_id, delegation_id),
    CHECK (octet_length(delegation_id) BETWEEN 1 AND 256),
    CHECK (octet_length(issuer_principal_id) BETWEEN 1 AND 256),
    CHECK (octet_length(delegatee_session_id) BETWEEN 1 AND 256),
    CHECK (project_id IS NULL OR octet_length(project_id) BETWEEN 1 AND 256),
    CHECK (task_id IS NULL OR octet_length(task_id) BETWEEN 1 AND 256),
    CHECK (octet_length(goal_id) BETWEEN 1 AND 256),
    CHECK (octet_length(policy_version) BETWEEN 1 AND 256),
    CHECK (octet_length(constitution_version) BETWEEN 1 AND 256),
    CHECK (revoked_by_principal_id IS NULL OR octet_length(revoked_by_principal_id) BETWEEN 1 AND 256),
    CHECK (revocation_reason IS NULL OR octet_length(revocation_reason) BETWEEN 1 AND 512)
);

CREATE INDEX IF NOT EXISTS delegation_events_by_delegation
    ON delegation_events (tenant_id, repository_id, delegation_id, stream_position ASC);

CREATE INDEX IF NOT EXISTS delegation_projections_by_status
    ON delegation_projections (tenant_id, repository_id, status, expires_at ASC);

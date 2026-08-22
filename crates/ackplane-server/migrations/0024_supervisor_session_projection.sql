-- ADR-0116 first server slice: durable, tenant/repository-scoped supervisor
-- registrations and session projections. Typed ingress and Bridge reads are
-- separate follow-on work; this schema never creates a command surface.
CREATE TABLE IF NOT EXISTS supervisor_registrations (
    tenant_id          TEXT        NOT NULL,
    repository_id      TEXT        NOT NULL,
    supervisor_id      TEXT        NOT NULL,
    node_id            TEXT        NOT NULL,
    supervisor_version TEXT        NOT NULL,
    protocol_version   TEXT        NOT NULL,
    capabilities       TEXT        NOT NULL,
    payload_digest     BYTEA       NOT NULL CHECK (octet_length(payload_digest) = 32),
    registered_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_heartbeat_at  BIGINT,
    PRIMARY KEY (tenant_id, repository_id, supervisor_id),
    CHECK (octet_length(tenant_id) BETWEEN 1 AND 256),
    CHECK (octet_length(repository_id) BETWEEN 1 AND 256),
    CHECK (octet_length(supervisor_id) BETWEEN 1 AND 256),
    CHECK (octet_length(node_id) BETWEEN 1 AND 256),
    CHECK (octet_length(supervisor_version) BETWEEN 1 AND 256),
    CHECK (octet_length(protocol_version) BETWEEN 1 AND 256),
    CHECK (octet_length(capabilities) BETWEEN 2 AND 4096),
    CHECK (last_heartbeat_at IS NULL OR last_heartbeat_at >= 0)
);

CREATE TABLE IF NOT EXISTS supervisor_sessions (
    tenant_id                 TEXT        NOT NULL,
    repository_id             TEXT        NOT NULL,
    session_id                TEXT        NOT NULL,
    supervisor_id             TEXT        NOT NULL,
    worker_id                 TEXT        NOT NULL,
    runtime                   SMALLINT    NOT NULL CHECK (runtime BETWEEN 1 AND 4),
    started_at                BIGINT      NOT NULL CHECK (started_at >= 0),
    current_state             SMALLINT    NOT NULL CHECK (current_state BETWEEN 1 AND 9),
    current_reason            SMALLINT    CHECK (current_reason BETWEEN 1 AND 7),
    current_occurred_at       BIGINT      NOT NULL CHECK (current_occurred_at >= 0),
    payload_digest            BYTEA       NOT NULL CHECK (octet_length(payload_digest) = 32),
    recorded_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, session_id),
    FOREIGN KEY (tenant_id, repository_id, supervisor_id)
        REFERENCES supervisor_registrations (tenant_id, repository_id, supervisor_id),
    CHECK (octet_length(session_id) BETWEEN 1 AND 256),
    CHECK (octet_length(worker_id) BETWEEN 1 AND 256)
);

CREATE TABLE IF NOT EXISTS supervisor_lifecycle_receipts (
    tenant_id         TEXT        NOT NULL,
    repository_id     TEXT        NOT NULL,
    session_id        TEXT        NOT NULL,
    receipt_position  BIGSERIAL   NOT NULL,
    supervisor_id     TEXT        NOT NULL,
    worker_id         TEXT        NOT NULL,
    occurred_at       BIGINT      NOT NULL CHECK (occurred_at >= 0),
    state             SMALLINT    NOT NULL CHECK (state BETWEEN 1 AND 9),
    reason            SMALLINT    CHECK (reason BETWEEN 1 AND 7),
    idempotency_key   TEXT        NOT NULL,
    payload_digest    BYTEA       NOT NULL CHECK (octet_length(payload_digest) = 32),
    recorded_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, session_id, receipt_position),
    UNIQUE (tenant_id, repository_id, session_id, idempotency_key),
    FOREIGN KEY (tenant_id, repository_id, session_id)
        REFERENCES supervisor_sessions (tenant_id, repository_id, session_id)
        ON DELETE CASCADE,
    CHECK (octet_length(idempotency_key) BETWEEN 1 AND 256)
);

CREATE INDEX IF NOT EXISTS supervisor_registrations_by_heartbeat
    ON supervisor_registrations (tenant_id, repository_id, last_heartbeat_at DESC NULLS LAST);

CREATE INDEX IF NOT EXISTS supervisor_sessions_by_state
    ON supervisor_sessions (tenant_id, repository_id, current_state, current_occurred_at DESC);

CREATE INDEX IF NOT EXISTS supervisor_lifecycle_receipts_by_session
    ON supervisor_lifecycle_receipts (tenant_id, repository_id, session_id, receipt_position ASC);

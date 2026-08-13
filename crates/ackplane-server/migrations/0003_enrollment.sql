-- ADR-0085: Ackplane owns node enrolment state. The current state is retained
-- for efficient reads, while every transition remains durable in the append-only
-- audit log below.
CREATE TABLE IF NOT EXISTS enrollment_requests (
    tenant_id              TEXT        NOT NULL,
    repository_id          TEXT        NOT NULL,
    request_id             TEXT        NOT NULL,
    proposed_node_id       TEXT        NOT NULL,
    display_name           TEXT        NOT NULL,
    public_key             BYTEA       NOT NULL,
    public_key_fingerprint TEXT        NOT NULL,
    requested_capabilities TEXT[]      NOT NULL,
    created_at             TIMESTAMPTZ NOT NULL,
    expires_at             TIMESTAMPTZ NOT NULL,
    state                  SMALLINT    NOT NULL,
    approved_fingerprint   TEXT,
    approved_capabilities  TEXT[],
    approved_at            TIMESTAMPTZ,
    approved_by            TEXT,
    PRIMARY KEY (tenant_id, repository_id, request_id)
);

-- The authoritative audit history. No service path updates or deletes a row:
-- a transition always appends actor, time, binding, and reason.
CREATE TABLE IF NOT EXISTS enrollment_transitions (
    transition_id          BIGSERIAL   PRIMARY KEY,
    tenant_id              TEXT        NOT NULL,
    repository_id          TEXT        NOT NULL,
    request_id             TEXT        NOT NULL,
    proposed_node_id       TEXT        NOT NULL,
    public_key_fingerprint TEXT        NOT NULL,
    state                  SMALLINT    NOT NULL,
    actor                  TEXT        NOT NULL,
    reason                 TEXT        NOT NULL,
    transitioned_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (tenant_id, repository_id, request_id)
        REFERENCES enrollment_requests (tenant_id, repository_id, request_id)
);

CREATE TABLE IF NOT EXISTS activation_challenges (
    tenant_id              TEXT        NOT NULL,
    repository_id          TEXT        NOT NULL,
    request_id             TEXT        NOT NULL,
    proposed_node_id       TEXT        NOT NULL,
    public_key_fingerprint TEXT        NOT NULL,
    nonce                  BYTEA       NOT NULL,
    issued_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at             TIMESTAMPTZ NOT NULL,
    consumed_at            TIMESTAMPTZ,
    PRIMARY KEY (tenant_id, repository_id, request_id),
    UNIQUE (nonce),
    FOREIGN KEY (tenant_id, repository_id, request_id)
        REFERENCES enrollment_requests (tenant_id, repository_id, request_id)
);

CREATE TABLE IF NOT EXISTS enrollment_receipts (
    enrollment_receipt_id  TEXT        PRIMARY KEY,
    tenant_id              TEXT        NOT NULL,
    repository_id          TEXT        NOT NULL,
    request_id             TEXT        NOT NULL,
    proposed_node_id       TEXT        NOT NULL,
    public_key_fingerprint TEXT        NOT NULL,
    activated_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, repository_id, request_id),
    FOREIGN KEY (tenant_id, repository_id, request_id)
        REFERENCES enrollment_requests (tenant_id, repository_id, request_id)
);

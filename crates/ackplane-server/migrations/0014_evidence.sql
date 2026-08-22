-- ADR-0106 decision 3: bounded, provenance-bearing evidence for the
-- Industrial Evidence Board. Evidence bodies remain at their source; this
-- domain stores only typed references and SHA-256 digests.
CREATE TABLE IF NOT EXISTS evidence_authentication_nonces (
    signing_key_id TEXT        NOT NULL,
    nonce          BYTEA       NOT NULL,
    consumed_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (signing_key_id, nonce)
);

CREATE TABLE IF NOT EXISTS evidence_records (
    tenant_id        TEXT        NOT NULL,
    repository_id    TEXT        NOT NULL,
    evidence_id      TEXT        NOT NULL,
    task_id          TEXT        NOT NULL,
    evidence_kind    SMALLINT    NOT NULL CHECK (evidence_kind BETWEEN 1 AND 5),
    source_ref       TEXT        NOT NULL,
    content_digest   BYTEA       NOT NULL CHECK (octet_length(content_digest) = 32),
    observed_at      TIMESTAMPTZ NOT NULL,
    agent_session_id TEXT        NOT NULL,
    recorded_by      TEXT        NOT NULL,
    recorded_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, evidence_id),
    CHECK (octet_length(task_id) BETWEEN 1 AND 256),
    CHECK (octet_length(source_ref) BETWEEN 1 AND 512),
    CHECK (octet_length(agent_session_id) BETWEEN 1 AND 256),
    CHECK (octet_length(recorded_by) BETWEEN 1 AND 256)
);

CREATE INDEX IF NOT EXISTS evidence_records_by_task
    ON evidence_records (
        tenant_id,
        repository_id,
        task_id,
        observed_at DESC,
        evidence_id ASC
    );

-- ADR-0115 item 5: escalation is a first-class durable state. A human
-- decision request is not a chat message, a log line, or an agent's own
-- assertion -- it is an append-only event with a checked projection, the
-- same durability guarantee ADR-0115's delegation grants already hold.
CREATE TABLE IF NOT EXISTS human_decision_stream_heads (
    tenant_id       TEXT        NOT NULL,
    repository_id   TEXT        NOT NULL,
    stream_position BIGINT      NOT NULL CHECK (stream_position >= 0),
    PRIMARY KEY (tenant_id, repository_id)
);

CREATE TABLE IF NOT EXISTS human_decision_events (
    tenant_id              TEXT        NOT NULL,
    repository_id          TEXT        NOT NULL,
    stream_position        BIGINT      NOT NULL CHECK (stream_position > 0),
    decision_id            TEXT        NOT NULL,
    event_kind             SMALLINT    NOT NULL CHECK (event_kind BETWEEN 1 AND 3),
    actor_principal_id     TEXT        NOT NULL,
    proposed_action        TEXT,
    target                 TEXT,
    reason                 TEXT,
    context_packet_digest  BYTEA,
    evidence_digest        BYTEA,
    alternatives           TEXT,
    safe_behavior          SMALLINT,
    related_delegation_id  TEXT,
    expires_at             TIMESTAMPTZ,
    rationale              TEXT,
    expected_prior_version INTEGER     NOT NULL CHECK (expected_prior_version >= 0),
    resulting_version      INTEGER     NOT NULL CHECK (resulting_version > 0),
    idempotency_key        TEXT        NOT NULL,
    payload_digest         BYTEA       NOT NULL CHECK (octet_length(payload_digest) = 32),
    schema_version         SMALLINT    NOT NULL CHECK (schema_version = 1),
    recorded_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, stream_position),
    UNIQUE (tenant_id, repository_id, idempotency_key),
    CHECK (octet_length(decision_id) BETWEEN 1 AND 256),
    CHECK (octet_length(actor_principal_id) BETWEEN 1 AND 256),
    CHECK (octet_length(idempotency_key) BETWEEN 1 AND 256),
    CHECK (context_packet_digest IS NULL OR octet_length(context_packet_digest) = 32),
    CHECK (evidence_digest IS NULL OR octet_length(evidence_digest) = 32)
);

CREATE TABLE IF NOT EXISTS human_decision_projections (
    tenant_id                 TEXT        NOT NULL,
    repository_id             TEXT        NOT NULL,
    decision_id               TEXT        NOT NULL,
    proposing_principal_id    TEXT        NOT NULL,
    proposed_action           TEXT        NOT NULL,
    target                    TEXT        NOT NULL,
    reason                    TEXT        NOT NULL,
    context_packet_digest     BYTEA       NOT NULL CHECK (octet_length(context_packet_digest) = 32),
    evidence_digest           BYTEA       NOT NULL CHECK (octet_length(evidence_digest) = 32),
    alternatives              TEXT        NOT NULL,
    safe_behavior             SMALLINT    NOT NULL CHECK (safe_behavior BETWEEN 1 AND 4),
    related_delegation_id      TEXT,
    requested_at               TIMESTAMPTZ NOT NULL,
    expires_at                 TIMESTAMPTZ NOT NULL,
    status                     SMALLINT    NOT NULL CHECK (status BETWEEN 1 AND 3),
    version                    INTEGER     NOT NULL CHECK (version > 0),
    source_event_position      BIGINT      NOT NULL CHECK (source_event_position > 0),
    resolved_at                TIMESTAMPTZ,
    resolved_by_principal_id   TEXT,
    resolution_rationale       TEXT,
    PRIMARY KEY (tenant_id, repository_id, decision_id),
    CHECK (octet_length(decision_id) BETWEEN 1 AND 256),
    CHECK (octet_length(proposing_principal_id) BETWEEN 1 AND 256),
    CHECK (octet_length(proposed_action) BETWEEN 1 AND 256),
    CHECK (octet_length(target) BETWEEN 1 AND 256),
    CHECK (octet_length(reason) BETWEEN 1 AND 512),
    CHECK (octet_length(alternatives) BETWEEN 1 AND 512),
    CHECK (related_delegation_id IS NULL OR octet_length(related_delegation_id) BETWEEN 1 AND 256),
    CHECK (resolved_by_principal_id IS NULL OR octet_length(resolved_by_principal_id) BETWEEN 1 AND 256),
    CHECK (resolution_rationale IS NULL OR octet_length(resolution_rationale) BETWEEN 1 AND 512)
);

CREATE INDEX IF NOT EXISTS human_decision_events_by_decision
    ON human_decision_events (tenant_id, repository_id, decision_id, stream_position ASC);

CREATE INDEX IF NOT EXISTS human_decision_projections_by_status
    ON human_decision_projections (tenant_id, repository_id, status, expires_at ASC);

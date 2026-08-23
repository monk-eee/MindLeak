-- ADR-0107: immutable typed directives persist before a future delivery
-- adapter sends them to an enrolled supervisor. The stored protobuf bytes are
-- bounded and remain a closed protocol envelope, never a shell or raw MCP payload.
CREATE TABLE IF NOT EXISTS directive_stream_heads (
    tenant_id            TEXT      NOT NULL,
    repository_id        TEXT      NOT NULL,
    node_id              TEXT      NOT NULL,
    agent_session_id     TEXT      NOT NULL,
    stream_position      BIGINT    NOT NULL DEFAULT 0 CHECK (stream_position >= 0),
    PRIMARY KEY (tenant_id, repository_id, node_id, agent_session_id),
    FOREIGN KEY (tenant_id, repository_id, agent_session_id)
        REFERENCES supervisor_sessions (tenant_id, repository_id, session_id)
        ON DELETE CASCADE,
    CHECK (octet_length(node_id) BETWEEN 1 AND 256),
    CHECK (octet_length(agent_session_id) BETWEEN 1 AND 256)
);

CREATE TABLE IF NOT EXISTS agent_directives (
    tenant_id             TEXT        NOT NULL,
    repository_id         TEXT        NOT NULL,
    directive_id          TEXT        NOT NULL,
    node_id               TEXT        NOT NULL,
    agent_session_id      TEXT        NOT NULL,
    project_id            TEXT        NOT NULL,
    directive_kind        SMALLINT    NOT NULL CHECK (directive_kind BETWEEN 1 AND 8),
    schema_version        TEXT        NOT NULL,
    issuing_principal_id  TEXT        NOT NULL,
    rationale             TEXT        NOT NULL,
    task_id               TEXT,
    goal_id               TEXT,
    context_packet_id     TEXT,
    created_at            TIMESTAMPTZ NOT NULL,
    expires_at            TIMESTAMPTZ NOT NULL,
    directive_sequence    BIGINT      NOT NULL CHECK (directive_sequence > 0),
    idempotency_key       TEXT        NOT NULL,
    request_digest        BYTEA       NOT NULL CHECK (octet_length(request_digest) = 32),
    payload_digest        BYTEA       NOT NULL CHECK (octet_length(payload_digest) = 32),
    required_capability   TEXT        NOT NULL,
    policy_refs           TEXT[]      NOT NULL DEFAULT '{}',
    knowledge_refs        TEXT[]      NOT NULL DEFAULT '{}',
    evidence_refs         TEXT[]      NOT NULL DEFAULT '{}',
    directive_payload     BYTEA       NOT NULL CHECK (octet_length(directive_payload) BETWEEN 1 AND 16384),
    recorded_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, directive_id),
    UNIQUE (tenant_id, repository_id, node_id, agent_session_id, directive_sequence),
    UNIQUE (tenant_id, repository_id, node_id, agent_session_id, idempotency_key),
    FOREIGN KEY (tenant_id, repository_id, node_id, agent_session_id)
        REFERENCES directive_stream_heads (tenant_id, repository_id, node_id, agent_session_id)
        ON DELETE CASCADE,
    CHECK (octet_length(directive_id) BETWEEN 1 AND 256),
    CHECK (octet_length(project_id) BETWEEN 1 AND 256),
    CHECK (octet_length(schema_version) BETWEEN 1 AND 256),
    CHECK (octet_length(issuing_principal_id) BETWEEN 1 AND 256),
    CHECK (octet_length(rationale) BETWEEN 1 AND 4096),
    CHECK (task_id IS NULL OR octet_length(task_id) BETWEEN 1 AND 256),
    CHECK (goal_id IS NULL OR octet_length(goal_id) BETWEEN 1 AND 256),
    CHECK (context_packet_id IS NULL OR octet_length(context_packet_id) BETWEEN 1 AND 256),
    CHECK (octet_length(idempotency_key) BETWEEN 1 AND 256),
    CHECK (octet_length(required_capability) BETWEEN 1 AND 256),
    CHECK (expires_at > created_at)
);

CREATE TABLE IF NOT EXISTS directive_receipts (
    tenant_id             TEXT        NOT NULL,
    repository_id         TEXT        NOT NULL,
    directive_id          TEXT        NOT NULL,
    receipt_position      BIGSERIAL   NOT NULL,
    node_id               TEXT        NOT NULL,
    agent_session_id      TEXT        NOT NULL,
    directive_sequence    BIGINT      NOT NULL CHECK (directive_sequence > 0),
    receipt_status        SMALLINT    NOT NULL CHECK (receipt_status BETWEEN 1 AND 5),
    receipt_reason        SMALLINT    NOT NULL CHECK (receipt_reason BETWEEN 1 AND 10),
    occurred_at           TIMESTAMPTZ NOT NULL,
    payload_digest        BYTEA       NOT NULL CHECK (octet_length(payload_digest) = 32),
    receipt_digest        BYTEA       NOT NULL CHECK (octet_length(receipt_digest) = 32),
    checkpoint_refs       TEXT[]      NOT NULL DEFAULT '{}',
    evidence_refs         TEXT[]      NOT NULL DEFAULT '{}',
    diagnostic            TEXT        NOT NULL DEFAULT '',
    receipt_payload       BYTEA       NOT NULL CHECK (octet_length(receipt_payload) BETWEEN 1 AND 16384),
    recorded_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, directive_id, receipt_position),
    UNIQUE (tenant_id, repository_id, directive_id, receipt_digest),
    FOREIGN KEY (tenant_id, repository_id, directive_id)
        REFERENCES agent_directives (tenant_id, repository_id, directive_id)
        ON DELETE CASCADE,
    CHECK (octet_length(node_id) BETWEEN 1 AND 256),
    CHECK (octet_length(agent_session_id) BETWEEN 1 AND 256),
    CHECK (octet_length(diagnostic) <= 4096)
);

CREATE INDEX IF NOT EXISTS agent_directives_by_target_sequence
    ON agent_directives (tenant_id, repository_id, node_id, agent_session_id, directive_sequence ASC);

CREATE INDEX IF NOT EXISTS agent_directives_by_expiry
    ON agent_directives (tenant_id, repository_id, expires_at ASC);

CREATE INDEX IF NOT EXISTS directive_receipts_by_directive
    ON directive_receipts (tenant_id, repository_id, directive_id, receipt_position ASC);

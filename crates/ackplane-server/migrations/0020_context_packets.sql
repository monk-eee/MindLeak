-- ADR-0114: durable storage for the Industrial ContextPacket protocol model
-- already shipped in `ackplane-protocol::context_packet` (decision 1). This
-- slice adds only the store: packets are immutable once written (decision 5)
-- and every delivery/use is a separate, attributed receipt (decision 7). The
-- compiler that produces packets (decisions 2-4), the authenticated request
-- service (decision 6), and Bridge inspection are later slices.
CREATE TABLE IF NOT EXISTS context_packets (
    tenant_id               TEXT    NOT NULL,
    repository_id           TEXT    NOT NULL,
    packet_id               TEXT    NOT NULL,
    task_id                 TEXT    NOT NULL,
    goal_id                 TEXT    NOT NULL,
    agent_session_id        TEXT    NOT NULL,
    compiler_version        TEXT    NOT NULL,
    -- Unix seconds, matching ackplane_protocol::context_packet::ContextPacket
    -- exactly -- stored as the same integer type, never a timestamptz cast,
    -- so no reader has to round-trip through a timezone-bearing type the
    -- wire contract does not have.
    issued_at               BIGINT  NOT NULL,
    expires_at              BIGINT  NOT NULL,
    ledger_position         BIGINT  NOT NULL,
    projection_position     BIGINT  NOT NULL,
    token_budget_requested  INTEGER NOT NULL,
    token_budget_used       INTEGER NOT NULL,
    -- The full validated ContextPacket, serialized -- selections/exclusions
    -- are read back through the same protocol type, never re-derived here.
    payload                 BYTEA       NOT NULL,
    payload_digest          BYTEA       NOT NULL,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, packet_id)
);

-- A supervisor's attributed observation of how it handled one packet
-- (decision 7). Many receipts may reference one packet; the packet itself
-- never changes because a receipt was recorded against it.
CREATE TABLE IF NOT EXISTS context_packet_use_receipts (
    tenant_id      TEXT      NOT NULL,
    repository_id  TEXT      NOT NULL,
    packet_id      TEXT      NOT NULL,
    receipt_id     BIGSERIAL NOT NULL,
    status         TEXT      NOT NULL,
    reason         TEXT,
    occurred_at    BIGINT      NOT NULL,
    recorded_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, packet_id, receipt_id),
    FOREIGN KEY (tenant_id, repository_id, packet_id)
        REFERENCES context_packets (tenant_id, repository_id, packet_id)
        ON DELETE CASCADE
);

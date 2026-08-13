-- ADR-0087: the Ackplane graph projection (clauses 1, 2, 6, 10).
--
-- Every table here is a rebuildable projection of accepted `ledger_records`
-- (clause 1): nothing writes to them except `Projector::rebuild`, which
-- replays a repository's committed `structural_fact` records in stream order.
-- Dropping and rebuilding a projection from the ledger must reproduce it
-- exactly; that is a required test, not a manual check.
CREATE TABLE IF NOT EXISTS projected_nodes (
    tenant_id     TEXT        NOT NULL,
    repository_id TEXT        NOT NULL,
    node_id       TEXT        NOT NULL,
    node_type     TEXT        NOT NULL,
    label         TEXT        NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (tenant_id, repository_id, node_id)
);

-- `base_weight` and `half_life_hours` are the projection's only stored decay
-- inputs; effective weight is always derived at query time from these plus
-- `updated_at` (clause 6). No column here ever holds a computed decay result.
CREATE TABLE IF NOT EXISTS projected_edges (
    tenant_id       TEXT             NOT NULL,
    repository_id   TEXT             NOT NULL,
    source_id       TEXT             NOT NULL,
    target_id       TEXT             NOT NULL,
    relation        TEXT             NOT NULL,
    base_weight     DOUBLE PRECISION NOT NULL,
    half_life_hours DOUBLE PRECISION NOT NULL,
    updated_at      TIMESTAMPTZ      NOT NULL,
    PRIMARY KEY (tenant_id, repository_id, source_id, target_id, relation)
);

-- Freshness a caller can see (clause 10): the highest ledger stream position
-- folded into the projection so far, and when that rebuild ran. A missing row
-- means "never projected", not position zero.
CREATE TABLE IF NOT EXISTS projection_state (
    tenant_id       TEXT        NOT NULL,
    repository_id   TEXT        NOT NULL,
    stream_position BIGINT      NOT NULL DEFAULT 0,
    projected_at    TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (tenant_id, repository_id)
);

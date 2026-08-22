-- ADR-0105 decision 6: the Industrial profile reports typed operational
-- telemetry (tool, transport, directive, storage, projection observations)
-- rather than forwarding local tool logs or raw terminal output. Bounded
-- measurements are stored as a JSON-encoded TEXT column rather than a
-- second child table: they are small (server-enforced count/name-length/
-- finite-value bounds), never queried individually, and read back only as
-- part of their own event -- exactly the shape a normalized child table
-- adds a join for without a benefit here (unlike knowledge_embeddings,
-- which pgvector must index). Plain TEXT, not JSONB: this workspace's
-- tokio-postgres dependency has no `with-serde_json-1` feature enabled, so
-- the store layer serializes/deserializes the JSON itself rather than
-- binding a `serde_json::Value` parameter.
CREATE TABLE IF NOT EXISTS telemetry_events (
    tenant_id        TEXT        NOT NULL,
    repository_id    TEXT        NOT NULL,
    telemetry_id     TEXT        NOT NULL,
    node_id          TEXT        NOT NULL,
    agent_session_id TEXT,
    kind             SMALLINT    NOT NULL,
    name             TEXT        NOT NULL,
    outcome          SMALLINT    NOT NULL,
    duration_ms      BIGINT      NOT NULL,
    occurred_at      TIMESTAMPTZ NOT NULL,
    recorded_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    measurements     TEXT        NOT NULL DEFAULT '[]',
    PRIMARY KEY (tenant_id, repository_id, telemetry_id)
);

-- Current-health and bucketed-series reads both filter by tenant/repository
-- and, optionally, kind/name, then order by occurred_at -- this index serves
-- both query shapes without a second index.
CREATE INDEX IF NOT EXISTS telemetry_events_lookup
    ON telemetry_events (tenant_id, repository_id, kind, name, occurred_at DESC);

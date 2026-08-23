-- ADR-0117: a durable delivery projection backs browser SSE replay. Cursors
-- are opaque per-tenant identifiers; sequence exists only for ordered reads.
CREATE TABLE IF NOT EXISTS live_feed_heads (
    tenant_id       TEXT        PRIMARY KEY,
    stream_position BIGINT      NOT NULL CHECK (stream_position >= 0)
);

CREATE TABLE IF NOT EXISTS live_feed_events (
    tenant_id            TEXT        NOT NULL,
    stream_position      BIGINT      NOT NULL CHECK (stream_position > 0),
    cursor               TEXT        NOT NULL,
    repository_id        TEXT,
    event_kind           SMALLINT    NOT NULL CHECK (event_kind BETWEEN 1 AND 9),
    resource_type        SMALLINT    NOT NULL CHECK (resource_type BETWEEN 1 AND 9),
    resource_id          TEXT        NOT NULL,
    change_kind          SMALLINT    NOT NULL CHECK (change_kind BETWEEN 1 AND 6),
    resource_version     BIGINT,
    source_ledger_position BIGINT,
    projection_position  BIGINT,
    projection_freshness SMALLINT    CHECK (projection_freshness BETWEEN 1 AND 3),
    snapshot_reload      BOOLEAN     NOT NULL DEFAULT FALSE,
    source_digest        BYTEA       NOT NULL CHECK (octet_length(source_digest) = 32),
    published_by         TEXT        NOT NULL,
    emitted_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, cursor),
    UNIQUE (tenant_id, stream_position),
    CHECK (octet_length(cursor) BETWEEN 1 AND 128),
    CHECK (repository_id IS NULL OR octet_length(repository_id) BETWEEN 1 AND 256),
    CHECK (octet_length(resource_id) BETWEEN 1 AND 256),
    CHECK (published_by <> '' AND octet_length(published_by) <= 256)
);

CREATE INDEX IF NOT EXISTS live_feed_events_by_tenant_repository_position
    ON live_feed_events (
        tenant_id,
        repository_id,
        stream_position ASC
    );

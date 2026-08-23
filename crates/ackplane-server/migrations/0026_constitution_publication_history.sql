-- ADR-0121 decision 1: every published Constitution version is immutable and
-- retained. This is ADDITIVE beside the existing `constitution_snapshots`/
-- `constitution_clauses` tables (0009_constitution.sql), which keep serving
-- `get_active`/`publish` exactly as before -- decision 8 requires expanding
-- without rewriting history, not replacing the current-snapshot projection
-- in this slice. Wiring `publish()` to also record here, and importing each
-- existing active snapshot as a baseline publication, are later slices.
CREATE TABLE IF NOT EXISTS constitution_publications (
    tenant_id        TEXT        NOT NULL,
    repository_id    TEXT        NOT NULL,
    version_id       TEXT        NOT NULL,
    schema_version   TEXT        NOT NULL,
    status           TEXT        NOT NULL,
    -- The full bounded publication content (status, schema_version, clause
    -- snapshot, source reference/digest), serialized -- read back through
    -- the same typed shape the store returns, never re-derived from the
    -- (mutable, single-row-per-repo) constitution_clauses table.
    payload          BYTEA       NOT NULL,
    payload_digest   BYTEA       NOT NULL,
    source_reference TEXT,
    source_digest    BYTEA,
    published_at     TIMESTAMPTZ NOT NULL,
    recorded_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, version_id)
);

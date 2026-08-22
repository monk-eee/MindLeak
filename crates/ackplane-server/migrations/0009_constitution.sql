-- ADR-0106 decision 3: Constitution is a read-only projection of a
-- repository's own authoritative local Lodestar constitution. Ackplane never
-- edits it -- a snapshot replaces the prior one for that tenant/repository,
-- the same "projection, not a second source of truth" rule
-- `knowledge`/`fleet` already follow for their own domains.
CREATE TABLE IF NOT EXISTS constitution_snapshots (
    tenant_id     TEXT        NOT NULL,
    repository_id TEXT        NOT NULL,
    version_id    TEXT        NOT NULL,
    version       BIGINT      NOT NULL,
    status        TEXT        NOT NULL,
    published_at  TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (tenant_id, repository_id)
);

-- One clause per row, replaced wholesale with its snapshot (never patched
-- clause-by-clause): a constitution version is immutable at the source
-- (SPEC-CONSTITUTION §10), so a new publish is a new, complete snapshot, not
-- an incremental diff this projection would have to reconcile.
CREATE TABLE IF NOT EXISTS constitution_clauses (
    tenant_id     TEXT   NOT NULL,
    repository_id TEXT   NOT NULL,
    clause_id     TEXT   NOT NULL,
    slug          TEXT   NOT NULL,
    kind          TEXT   NOT NULL,
    title         TEXT   NOT NULL,
    statement     TEXT   NOT NULL,
    status        TEXT   NOT NULL,
    consequence   TEXT,
    scope         TEXT,
    rationale     TEXT,
    PRIMARY KEY (tenant_id, repository_id, clause_id),
    FOREIGN KEY (tenant_id, repository_id)
        REFERENCES constitution_snapshots (tenant_id, repository_id)
        ON DELETE CASCADE
);

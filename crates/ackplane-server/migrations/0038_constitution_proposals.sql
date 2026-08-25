-- ADR-0126 decision 1: an append-only record of a suggested constitution
-- clause change, originated from the Bridge. Distinct from
-- constitution_publications (0026_constitution_publication_history.sql):
-- a publication is an authoritative fact about what a repository's own
-- Lodestar already activated; a proposal is only ever a suggestion, and
-- carries no authority of its own (decision 2/7 -- this table is never
-- read by anything that decides what is active).
CREATE TABLE IF NOT EXISTS constitution_proposals (
    tenant_id     TEXT        NOT NULL,
    repository_id TEXT        NOT NULL,
    proposal_id   TEXT        NOT NULL,
    -- The suggested clause change, in the exact ClauseSnapshot shape the
    -- existing read projection already returns (decision 1) -- no new
    -- clause type, so a diff against the active snapshot has nothing new
    -- to reconcile.
    kind          TEXT        NOT NULL,
    slug          TEXT        NOT NULL,
    title         TEXT        NOT NULL,
    statement     TEXT        NOT NULL,
    consequence   TEXT,
    scope         TEXT,
    rationale     TEXT,
    author        TEXT        NOT NULL,
    -- 'proposed' or 'withdrawn' (decision 3). Adoption is never recorded
    -- here: it is read-only pattern matching over this table and
    -- constitution_publications, correlated at read time, not a status
    -- this table's own writer sets.
    status        TEXT        NOT NULL DEFAULT 'proposed',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, proposal_id)
);

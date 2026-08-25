-- ADR-0113 decision 1: a knowledge candidate becomes active only through an
-- explicit, authorized activation -- never implicitly at record time. Rows
-- recorded before this migration were already being recalled/paged as
-- established guidance, so they backfill as Active (2); only a statement
-- recorded going forward (KnowledgeStore::record) starts as Candidate (1),
-- via an explicit application-level insert value, not this column's default.
ALTER TABLE knowledge
    ADD COLUMN IF NOT EXISTS lifecycle_state SMALLINT NOT NULL DEFAULT 2;

ALTER TABLE knowledge
    ADD CONSTRAINT knowledge_lifecycle_state_check CHECK (lifecycle_state BETWEEN 1 AND 2);

-- One immutable receipt per accepted Candidate -> Active transition (ADR-0113
-- decision 7): the actor, an optional reason, and when. There is exactly one
-- possible transition today, so prior/new state are not columns here --
-- unlike industrial_design_decisions, which records an open-ended sequence of
-- decision kinds -- but the row itself is never updated or deleted once
-- written, the same append-only contract as knowledge_reconfirmations.
CREATE TABLE IF NOT EXISTS knowledge_activations (
    tenant_id       TEXT        NOT NULL,
    repository_id   TEXT        NOT NULL,
    knowledge_id    TEXT        NOT NULL,
    activation_id   TEXT        NOT NULL,
    authorized_by   TEXT        NOT NULL,
    reason          TEXT,
    activated_at    TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (tenant_id, repository_id, activation_id),
    FOREIGN KEY (tenant_id, repository_id, knowledge_id)
        REFERENCES knowledge (tenant_id, repository_id, knowledge_id)
);

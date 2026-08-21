-- ADR-0022: reconfirmation is durable evidence, not an untracked timestamp
-- rewrite. Every event names the authenticated node and corroborating source
-- that reset the knowledge decay clock.
CREATE TABLE IF NOT EXISTS knowledge_reconfirmations (
    tenant_id          TEXT        NOT NULL,
    repository_id      TEXT        NOT NULL,
    knowledge_id       TEXT        NOT NULL,
    reconfirmation_id  TEXT        NOT NULL,
    evidence_ref       TEXT        NOT NULL,
    reconfirmed_by     TEXT        NOT NULL,
    reconfirmed_at     TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (tenant_id, repository_id, knowledge_id, reconfirmation_id),
    FOREIGN KEY (tenant_id, repository_id, knowledge_id)
        REFERENCES knowledge (tenant_id, repository_id, knowledge_id)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS knowledge_reconfirmations_latest
    ON knowledge_reconfirmations
       (tenant_id, repository_id, knowledge_id, reconfirmed_at DESC, reconfirmation_id DESC);

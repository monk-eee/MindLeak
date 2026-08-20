-- ADR-0106 decision 3: knowledge is one of the PostgreSQL-backed Industrial
-- domains. This is its first slice: record a learned-knowledge statement,
-- recall it ranked by pgvector similarity, and retire it with an attributed
-- reason. Embeddings are ranked through pgvector's own `<=>` operator, not
-- pulled into application memory for a cosine loop over a BLOB -- the exact
-- SQLite scaling limit this domain exists to not repeat (lodestar-core's own
-- knowledge_embeddings table does the latter, by necessity: SQLite has no
-- native vector type).
CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS knowledge (
    tenant_id       TEXT             NOT NULL,
    repository_id   TEXT             NOT NULL,
    knowledge_id    TEXT             NOT NULL,
    content         TEXT             NOT NULL,
    source_ref      TEXT,
    half_life_hours DOUBLE PRECISION NOT NULL,
    confirmed_at    TIMESTAMPTZ      NOT NULL,
    retired_at      TIMESTAMPTZ,
    retired_reason  TEXT,
    retired_by      TEXT,
    PRIMARY KEY (tenant_id, repository_id, knowledge_id)
);

-- Decoupled from `knowledge` by (knowledge_id, model), the same way
-- lodestar-core's SQLite knowledge_embeddings table is: a statement may be
-- re-embedded under a new model without losing its history, and recalling
-- under a model it was never embedded for degrades to recency ordering
-- rather than erroring. `vector(768)` is nomic-embed-text's dimension, the
-- default embedder this domain shares with the local planes -- a fixed
-- dimension is what lets an ivfflat index exist at all; a second model
-- dimension needs its own column or table, not a redesign of this one.
CREATE TABLE IF NOT EXISTS knowledge_embeddings (
    tenant_id     TEXT      NOT NULL,
    repository_id TEXT      NOT NULL,
    knowledge_id  TEXT      NOT NULL,
    model         TEXT      NOT NULL,
    embedding     vector(768) NOT NULL,
    PRIMARY KEY (tenant_id, repository_id, knowledge_id, model),
    FOREIGN KEY (tenant_id, repository_id, knowledge_id)
        REFERENCES knowledge (tenant_id, repository_id, knowledge_id)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS knowledge_embeddings_ivfflat
    ON knowledge_embeddings USING ivfflat (embedding vector_cosine_ops)
    WITH (lists = 100);

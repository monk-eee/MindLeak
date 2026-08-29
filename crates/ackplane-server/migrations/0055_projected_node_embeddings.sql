-- ADR-0140: a pgvector-backed embeddings table scoped to the ledger-derived
-- projected_nodes projection (ADR-0087), not the curated, human-governed
-- knowledge/knowledge_embeddings domain (ADR-0113). Structurally identical to
-- 0007_knowledge.sql's knowledge_embeddings -- same (node_id, model) decoupling
-- so a node may be re-embedded under a new model without losing history, same
-- vector(768) dimension (nomic-embed-text, the default embedder both planes
-- share) -- applied to the projection instead of the curated domain.
--
-- This migration is schema only (ADR-0140 decision 1). Population (decision 2:
-- an optional second pass over projected_nodes, never a second writer) and the
-- ranking pipeline (decision 3: kind_prior + distinctive_cut, not a bare
-- pgvector distance ORDER BY) are deliberately separate, larger slices.
--
-- CREATE EXTENSION IF NOT EXISTS is idempotent and 0007_knowledge.sql already
-- ensures pgvector exists by the time this later-numbered migration runs, but
-- declaring it again here keeps this file correct in isolation rather than
-- depending on migration order for a extension neither file owns exclusively.
CREATE EXTENSION IF NOT EXISTS vector;

CREATE TABLE IF NOT EXISTS projected_node_embeddings (
    tenant_id     TEXT        NOT NULL,
    repository_id TEXT        NOT NULL,
    node_id       TEXT        NOT NULL,
    model         TEXT        NOT NULL,
    embedding     vector(768) NOT NULL,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id, repository_id, node_id, model),
    FOREIGN KEY (tenant_id, repository_id, node_id)
        REFERENCES projected_nodes (tenant_id, repository_id, node_id)
        ON DELETE CASCADE
);

-- Supports the ranking pipeline's first stage: a bounded pgvector distance
-- candidate query, before kind_prior/distinctive_cut ever run in application
-- code (ADR-0140 decision 3). Ranking itself is not implemented by this slice.
CREATE INDEX IF NOT EXISTS projected_node_embeddings_ivfflat
    ON projected_node_embeddings USING ivfflat (embedding vector_cosine_ops)
    WITH (lists = 100);

-- MindLeak temporal context graph schema.
-- Nodes are entities (symbols, artifacts, executions, intents).
-- Edges are directional, decay-weighted relationships.

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS nodes (
    id               TEXT PRIMARY KEY,        -- e.g. "artifact:src/auth.ts"
    type             TEXT NOT NULL,           -- symbol | artifact | execution | intent | agent | package
    label            TEXT NOT NULL,           -- human-readable name
    content          TEXT,                    -- code snippet, commit msg, or log excerpt
    created_at       INTEGER NOT NULL,        -- unix seconds
    last_accessed_at INTEGER NOT NULL         -- unix seconds
);

CREATE TABLE IF NOT EXISTS edges (
    source_id       TEXT NOT NULL,
    target_id       TEXT NOT NULL,
    relation        TEXT NOT NULL,            -- modified | failed_on | contains | refactored | relates_to | calls | observed | imports | extends | implements | depends_on
    weight          REAL NOT NULL DEFAULT 1.0,
    half_life_hours REAL NOT NULL DEFAULT 48.0,
    updated_at      INTEGER NOT NULL,
    first_seen           INTEGER NOT NULL DEFAULT 0,   -- earliest reinforcement (signal span anchor)
    reinforcement_count  INTEGER NOT NULL DEFAULT 1,   -- times reinforced (signal proxy; ADR-0005)
    owner_id        TEXT,                              -- artifact owning a structural snapshot (ADR-0007)
    PRIMARY KEY (source_id, target_id, relation),
    FOREIGN KEY (source_id) REFERENCES nodes(id) ON DELETE CASCADE,
    FOREIGN KEY (target_id) REFERENCES nodes(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_id, weight);
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_id);
CREATE INDEX IF NOT EXISTS idx_nodes_type   ON nodes(type);

-- Import resolution may need a best-guess artifact before that file is ingested.
-- Provenance stays internal so a real ingest can promote the same artifact id.
CREATE TABLE IF NOT EXISTS artifact_stubs (
    node_id TEXT PRIMARY KEY,
    FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS artifact_stub_candidates (
    stub_id      TEXT NOT NULL,
    candidate_id TEXT NOT NULL,
    PRIMARY KEY (stub_id, candidate_id),
    FOREIGN KEY (stub_id) REFERENCES artifact_stubs(node_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_stub_candidates_candidate
    ON artifact_stub_candidates(candidate_id);

-- Cross-process optional maintenance coordination. One guarded row prevents
-- duplicate model spend when multiple MCP processes share a graph.
CREATE TABLE IF NOT EXISTS maintenance_leases (
    name             TEXT PRIMARY KEY,
    owner            TEXT NOT NULL,
    lease_expires_at INTEGER NOT NULL,
    last_attempt_at  INTEGER NOT NULL
);

-- Full-text index over node label + content for semantic-ish seed lookup.
CREATE VIRTUAL TABLE IF NOT EXISTS nodes_fts USING fts5(
    id UNINDEXED,
    label,
    content,
    tokenize = 'porter unicode61'
);

CREATE TRIGGER IF NOT EXISTS nodes_ai AFTER INSERT ON nodes BEGIN
    INSERT INTO nodes_fts(id, label, content)
    VALUES (new.id, new.label, coalesce(new.content, ''));
END;

CREATE TRIGGER IF NOT EXISTS nodes_ad AFTER DELETE ON nodes BEGIN
    DELETE FROM nodes_fts WHERE id = old.id;
END;

CREATE TRIGGER IF NOT EXISTS nodes_au AFTER UPDATE ON nodes BEGIN
    DELETE FROM nodes_fts WHERE id = old.id;
    INSERT INTO nodes_fts(id, label, content)
    VALUES (new.id, new.label, coalesce(new.content, ''));
END;

-- Lodestar (Intent Plane) schema — durable spec/constitution + coordination.
-- Distinct store from the MindLeak decay graph (ADR-0004): this plane persists.

PRAGMA foreign_keys = ON;

-- Goals: the constitution. Durable and versioned. Superseding creates a new row
-- and marks the old one 'superseded' (never edited in place).
CREATE TABLE IF NOT EXISTS goals (
    id            TEXT PRIMARY KEY,        -- e.g. "goal:zero-token-write-path"
    slug          TEXT NOT NULL,           -- stable identity across versions
    kind          TEXT NOT NULL,           -- objective | constraint | invariant
    title         TEXT NOT NULL,
    statement     TEXT NOT NULL,           -- the normative text
    status        TEXT NOT NULL,           -- draft | active | superseded
    version       INTEGER NOT NULL DEFAULT 1,
    parent_id     TEXT,                    -- goal hierarchy
    superseded_by TEXT,                    -- id of the version that replaced this
    reason        TEXT,                    -- why this version was written
    created_at    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_goals_status ON goals(status);
CREATE INDEX IF NOT EXISTS idx_goals_slug   ON goals(slug);

-- Tasks: the executive ledger. Live coordination state; not versioned.
CREATE TABLE IF NOT EXISTS tasks (
    id               TEXT PRIMARY KEY,     -- e.g. "task:9f3a1c"
    goal_id          TEXT NOT NULL,        -- the goal this task serves
    parent_task_id   TEXT,                 -- decomposition tree
    title            TEXT NOT NULL,
    acceptance       TEXT NOT NULL DEFAULT '',
    status           TEXT NOT NULL,        -- open|claimed|in_review|done|blocked|abandoned
    owner            TEXT,                 -- agent id holding the claim
    claim_started_at INTEGER,              -- start of the current owner's evidence window
    lease_expires_at INTEGER,              -- unix seconds; past this is reclaimable
    blocked_by       TEXT,                 -- optional task id
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_goal   ON tasks(goal_id);

-- Seam to MindLeak: which code nodes realise a goal. node_id is an opaque
-- MindLeak id string (e.g. "artifact:src/auth.rs"); no cross-DB FK.
CREATE TABLE IF NOT EXISTS goal_code (
    goal_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    mode    TEXT NOT NULL DEFAULT 'governed', -- governed | forbid_change
    PRIMARY KEY (goal_id, node_id)
);
CREATE INDEX IF NOT EXISTS idx_goal_code_node ON goal_code(node_id);

-- Conformance audit trail.
CREATE TABLE IF NOT EXISTS conformance (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id    TEXT,
    evidence_schema_version INTEGER,
    evidence   TEXT,
    verdict    TEXT NOT NULL,             -- aligned | drift | violation | needs_human
    findings   TEXT NOT NULL DEFAULT '',
    checked_at INTEGER NOT NULL
);

-- Learned knowledge: consolidated regularities. Durable but not immortal —
-- revalidated by fresh evidence via effective_weight over confirmed_at (ADR-0005).
CREATE TABLE IF NOT EXISTS knowledge (
    id              TEXT PRIMARY KEY,      -- e.g. "knowledge:1a2b3c"
    statement       TEXT NOT NULL,
    evidence        TEXT NOT NULL DEFAULT '', -- JSON provenance: nodes + count + span
    weight          REAL NOT NULL DEFAULT 1.0,
    half_life_hours REAL NOT NULL DEFAULT 720.0, -- ~30 days
    confirmed_at    INTEGER NOT NULL,      -- last reconfirmation (decay clock)
    created_at      INTEGER NOT NULL
);

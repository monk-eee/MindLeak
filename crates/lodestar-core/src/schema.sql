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

-- Design items: an ADR under human review (ADR-0023). The taint is the ADR's
-- own 'proposed' status; a human accept/reject is the completion path for design
-- work (no code conformance -- there is no code to conform to). A proposed item
-- is not claimable and never appears in next_task/the executive board. Rejection
-- is durable, never deleted (archive-not-delete, ADR-0019).
CREATE TABLE IF NOT EXISTS design_items (
    id           TEXT PRIMARY KEY,     -- e.g. "design:0023-design-board-accept-bridge"
    adr_path     TEXT NOT NULL,        -- docs/adr/NNNN-....md (forward slashes)
    title        TEXT NOT NULL,
    summary      TEXT NOT NULL DEFAULT '',
    status       TEXT NOT NULL,        -- proposed | accepted | rejected
    proposed_by  TEXT,                 -- agent that registered it (may not decide it)
    decided_by   TEXT,                 -- human that accepted/rejected it
    reason       TEXT,                 -- acceptance/rejection rationale
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    promotion_status TEXT NOT NULL DEFAULT 'not_required',
    spawned_goal_id TEXT               -- objective selected during promotion
);
CREATE INDEX IF NOT EXISTS idx_design_items_status ON design_items(status);

-- Durable provenance from an accepted design to the goals/tasks materialized by
-- promotion. These links make retries resolvable without re-running planning.
CREATE TABLE IF NOT EXISTS design_goal_links (
    design_id TEXT NOT NULL,
    goal_id   TEXT NOT NULL,
    role      TEXT NOT NULL,            -- objective | constraint | invariant
    position  INTEGER NOT NULL,
    PRIMARY KEY (design_id, goal_id),
    FOREIGN KEY (design_id) REFERENCES design_items(id) ON DELETE CASCADE,
    FOREIGN KEY (goal_id) REFERENCES goals(id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS design_task_links (
    design_id TEXT NOT NULL,
    task_id   TEXT NOT NULL,
    position  INTEGER NOT NULL,
    PRIMARY KEY (design_id, task_id),
    FOREIGN KEY (design_id) REFERENCES design_items(id) ON DELETE CASCADE,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);

-- Tasks: the executive ledger. Live coordination state; not versioned.
CREATE TABLE IF NOT EXISTS tasks (
    id               TEXT PRIMARY KEY,     -- e.g. "task:9f3a1c"
    goal_id          TEXT NOT NULL,        -- the goal this task serves
    parent_task_id   TEXT,                 -- decomposition tree
    title            TEXT NOT NULL,
    acceptance       TEXT NOT NULL DEFAULT '',
    status           TEXT NOT NULL,        -- open|claimed|needs_input|paused|in_review|done|blocked|abandoned
    owner            TEXT,                 -- agent id holding the claim
    claim_started_at INTEGER,              -- start of the current owner's evidence window
    lease_expires_at INTEGER,              -- unix seconds; past this is reclaimable
    blocked_by       TEXT,                 -- optional task id
    parked_at        INTEGER,              -- when parked (needs_input/paused); reclaimable after a grace
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_goal   ON tasks(goal_id);
CREATE INDEX IF NOT EXISTS idx_tasks_blocked_by ON tasks(blocked_by);

-- Optional advisory scope declared atomically with a task claim (ADR-0024).
-- Values are workspace-relative path globs or opaque MindLeak symbol ids. They
-- inform pre-flight checks only; they are not locks.
CREATE TABLE IF NOT EXISTS task_scopes (
    task_id TEXT NOT NULL,
    kind    TEXT NOT NULL,              -- path | symbol
    value   TEXT NOT NULL,
    PRIMARY KEY (task_id, kind, value),
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_task_scopes_value ON task_scopes(kind, value);

-- Durable, append-only question/answer thread for needs_input tasks (ADR-0020):
-- an agent's question awaiting a human answer. Never edited or deleted.
CREATE TABLE IF NOT EXISTS task_qa (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id    TEXT NOT NULL,
    kind       TEXT NOT NULL,             -- question | answer
    body       TEXT NOT NULL,
    author     TEXT NOT NULL,             -- agent id (question) or answerer (answer)
    created_at INTEGER NOT NULL,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_task_qa_task ON task_qa(task_id);

-- Durable progressive-handoff lineage. `tasks.blocked_by` is cleared when the
-- successor opens; this table retains the one-to-one chain invariant.
CREATE TABLE IF NOT EXISTS task_handoffs (
    predecessor_id TEXT PRIMARY KEY,
    successor_id   TEXT NOT NULL UNIQUE,
    created_at     INTEGER NOT NULL,
    FOREIGN KEY (predecessor_id) REFERENCES tasks(id) ON DELETE CASCADE,
    FOREIGN KEY (successor_id) REFERENCES tasks(id) ON DELETE CASCADE
);

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

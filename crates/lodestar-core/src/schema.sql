-- Lodestar (Intent Plane) schema — durable spec/constitution + coordination.
-- Distinct store from the MindLeak decay graph (ADR-0004): this plane persists.

PRAGMA foreign_keys = ON;

-- Constitution versions: the immutable, attributed policy snapshot that
-- authorises verdicts (SPEC-CONSTITUTION §10). An amendment writes a new
-- version; prior conformance records keep the version they were judged under.
CREATE TABLE IF NOT EXISTS constitution_versions (
    id               TEXT PRIMARY KEY,     -- e.g. "constitution:v1"
    version          INTEGER NOT NULL,
    project_identity TEXT,
    purpose          TEXT,
    preamble         TEXT,
    status           TEXT NOT NULL,        -- draft | active | superseded
    created_by       TEXT,
    created_at       INTEGER NOT NULL,
    activated_by     TEXT,
    activated_at     INTEGER
);
CREATE INDEX IF NOT EXISTS idx_constitution_status ON constitution_versions(status);

-- Goals: the clauses of the constitution. Durable and versioned. Superseding
-- creates a new row and marks the old one 'superseded' (never edited in place).
-- The enforcement fields (scope, evidence_contract, consequence) stay NULL until
-- completed; an incomplete clause is review-only and can never hard-block.
CREATE TABLE IF NOT EXISTS goals (
    id            TEXT PRIMARY KEY,        -- e.g. "goal:zero-token-write-path"
    slug          TEXT NOT NULL,           -- stable identity across versions
    kind          TEXT NOT NULL,           -- objective | constraint | invariant | principle
    title         TEXT NOT NULL,
    statement     TEXT NOT NULL,           -- the normative text
    status        TEXT NOT NULL,           -- draft | active | superseded
    version       INTEGER NOT NULL DEFAULT 1,
    parent_id     TEXT,                    -- goal hierarchy
    superseded_by TEXT,                    -- id of the version that replaced this
    reason        TEXT,                    -- why this version was written
    created_at    INTEGER NOT NULL,
    constitution_version TEXT,             -- id of the owning constitution version
    rationale            TEXT,             -- why the clause exists
    scope                TEXT,             -- where the clause applies
    evidence_contract    TEXT,             -- what evidence satisfies it
    consequence          TEXT,             -- advise | review | block
    waivable             INTEGER NOT NULL DEFAULT 0,
    waiver_authority     TEXT,             -- authority required to waive
    origin               TEXT NOT NULL DEFAULT 'local'  -- local | pack | discovered
);
CREATE INDEX IF NOT EXISTS idx_goals_status ON goals(status);
CREATE INDEX IF NOT EXISTS idx_goals_slug   ON goals(slug);

-- Policy packs are immutable, versioned inputs to constitutional drafting
-- (SPEC-CONSTITUTION section 6). Adoption copies a clause into goals and records
-- immutable provenance; packs are never live dependencies of local policy.
CREATE TABLE IF NOT EXISTS policy_packs (
    pack_id      TEXT NOT NULL,
    version      TEXT NOT NULL,
    digest       TEXT NOT NULL,
    title        TEXT NOT NULL,
    description  TEXT NOT NULL,
    content_json TEXT NOT NULL,
    created_at   INTEGER NOT NULL,
    PRIMARY KEY (pack_id, version)
);

CREATE TABLE IF NOT EXISTS policy_pack_conflicts (
    pack_id             TEXT NOT NULL,
    pack_version        TEXT NOT NULL,
    conflicting_pack_id TEXT NOT NULL,
    reason              TEXT NOT NULL,
    PRIMARY KEY (pack_id, pack_version, conflicting_pack_id),
    FOREIGN KEY (pack_id, pack_version)
        REFERENCES policy_packs(pack_id, version) ON DELETE CASCADE
);

-- One durable review record per pack clause and constitution context. A
-- rejection remains here so bootstrap cannot silently re-propose it.
CREATE TABLE IF NOT EXISTS pack_clause_proposals (
    id                   TEXT PRIMARY KEY,
    pack_id              TEXT NOT NULL,
    pack_version         TEXT NOT NULL,
    pack_digest          TEXT NOT NULL,
    constitution_version TEXT NOT NULL DEFAULT '',
    clause_key           TEXT NOT NULL,
    clause_json          TEXT NOT NULL,
    disposition          TEXT,
    reviewed_by          TEXT,
    review_reason        TEXT,
    reviewed_at          INTEGER,
    adopted_goal_id      TEXT,
    created_at           INTEGER NOT NULL,
    UNIQUE (pack_id, pack_version, constitution_version, clause_key),
    FOREIGN KEY (pack_id, pack_version)
        REFERENCES policy_packs(pack_id, version) ON DELETE CASCADE,
    FOREIGN KEY (adopted_goal_id) REFERENCES goals(id) ON DELETE SET NULL
);

-- Original source survives independently after adoption. An upstream pack
-- version can therefore never rewrite active local policy.
CREATE TABLE IF NOT EXISTS pack_clause_provenance (
    goal_id       TEXT PRIMARY KEY,
    pack_id       TEXT NOT NULL,
    pack_version  TEXT NOT NULL,
    pack_digest   TEXT NOT NULL,
    clause_key    TEXT NOT NULL,
    clause_json   TEXT NOT NULL,
    FOREIGN KEY (goal_id) REFERENCES goals(id) ON DELETE CASCADE
);

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
    materialization_revision INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_design_items_status ON design_items(status);

-- Append-only record of every reviewed materialization. The current link tables
-- below are a projection of the latest revision; earlier reviewed plans remain
-- durable here even when a human repairs a bad materialization.
CREATE TABLE IF NOT EXISTS design_materializations (
    design_id  TEXT NOT NULL,
    revision   INTEGER NOT NULL,
    mode       TEXT NOT NULL,            -- create | link | no_work
    plan_json  TEXT NOT NULL,
    rationale  TEXT,
    actor      TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (design_id, revision),
    FOREIGN KEY (design_id) REFERENCES design_items(id) ON DELETE CASCADE
);

-- Durable provenance from an accepted design to the goals/tasks materialized by
-- promotion. These links are the latest materialization's current projection.
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

-- Append-only ownership recovery audit (ADR-0030). Recovery never rewrites
-- history: it records the full prior claim window before assigning a fresh one.
CREATE TABLE IF NOT EXISTS task_claim_transfers (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id               TEXT NOT NULL,
    from_owner            TEXT NOT NULL,
    to_owner              TEXT NOT NULL,
    recovered_by          TEXT NOT NULL,
    reason                TEXT NOT NULL,
    from_status           TEXT NOT NULL,
    from_claim_started_at INTEGER,
    from_lease_expires_at INTEGER,
    from_parked_at        INTEGER,
    to_claim_started_at   INTEGER NOT NULL,
    transferred_at        INTEGER NOT NULL,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_task_claim_transfers_task
    ON task_claim_transfers(task_id, id);

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

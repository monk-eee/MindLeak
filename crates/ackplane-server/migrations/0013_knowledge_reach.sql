-- A knowledge statement may name the repository nodes and optional goal it
-- reaches. These are typed domain identifiers, not a copy of local SQLite
-- graph tables: Ackplane stores only the declared scope its enrolled node
-- published with the statement.
ALTER TABLE knowledge
    ADD COLUMN IF NOT EXISTS reach_node_ids TEXT[] NOT NULL DEFAULT '{}';

ALTER TABLE knowledge
    ADD COLUMN IF NOT EXISTS reach_goal_id TEXT;

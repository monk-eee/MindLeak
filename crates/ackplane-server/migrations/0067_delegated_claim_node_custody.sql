ALTER TABLE delegated_claims
    ADD COLUMN IF NOT EXISTS owner_node_id TEXT
    CHECK (owner_node_id IS NULL OR btrim(owner_node_id) <> '');

ALTER TABLE delegated_claim_history
    ADD COLUMN IF NOT EXISTS requested_node_id TEXT
    CHECK (requested_node_id IS NULL OR btrim(requested_node_id) <> '');

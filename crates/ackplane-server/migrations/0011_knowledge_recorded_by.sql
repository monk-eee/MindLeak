-- Authenticated knowledge provenance is distinct from the source reference:
-- source_ref identifies the evidence a statement cites, while recorded_by
-- identifies the enrolled node that made the statement. Existing rows stay
-- NULL because inventing an actor during migration would falsify history.
ALTER TABLE knowledge
    ADD COLUMN IF NOT EXISTS recorded_by TEXT;

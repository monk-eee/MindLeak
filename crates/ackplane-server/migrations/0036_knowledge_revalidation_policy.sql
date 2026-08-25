-- ADR-0113 decision 4: "Effective relevance is derived at query time from
-- confirmed time, half-life, lifecycle state, and any policy-defined
-- revalidation rule." A policy MAY define a revalidation rule for a
-- knowledge statement -- the ADR's own wording makes this optional, and no
-- policy-authoring surface exists yet (deliberately out of scope for this
-- change, same as the read-only Bridge view this migration supports: no
-- mutation route, no policy-authoring UI). This column is the value such a
-- rule would set once one exists; nothing writes it yet, so it is NULL for
-- every row today -- the revalidation-queue classification this migration
-- supports must therefore treat NULL correctly, not just a populated value.
ALTER TABLE knowledge
    ADD COLUMN IF NOT EXISTS revalidate_after_hours DOUBLE PRECISION;

ALTER TABLE knowledge
    ADD CONSTRAINT knowledge_revalidate_after_hours_positive
    CHECK (revalidate_after_hours IS NULL OR revalidate_after_hours > 0);

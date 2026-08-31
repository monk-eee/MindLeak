-- ADR-0142 decision 4: a caller may still supply a bounded, optional
-- display label ("who to show in the UI"), stored separately from and
-- never substituted for the verified principal now recorded as the
-- authoritative actor/author (gaps.d/design-constitution-display-label-
-- not-stored-separately.md). Additive on two existing tables; the
-- industrial_designs/industrial_design_decisions half of this same gap is
-- deferred to a follow-up, since design_store.rs carries a live ADR-0143
-- pool-migration claim at the time this lands.
ALTER TABLE constitution_proposals
    ADD COLUMN IF NOT EXISTS display_label TEXT;

ALTER TABLE industrial_design_materializations
    ADD COLUMN IF NOT EXISTS display_label TEXT;

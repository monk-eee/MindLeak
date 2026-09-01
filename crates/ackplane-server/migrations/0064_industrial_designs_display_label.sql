-- ADR-0142 decision 4: a caller may still supply a bounded, optional
-- display label ("who to show in the UI"), stored separately from and
-- never substituted for the verified principal now recorded as the
-- authoritative actor/author (gaps.d/design-constitution-display-label-
-- not-stored-separately.md). This is the third and final table of that
-- same decision -- constitution_proposals (migration 0060) and
-- industrial_design_materializations (migration 0061) already closed --
-- deferred from that original pass because design_store.rs carried a
-- live ADR-0143 pool-migration claim at the time; that claim is now done.
ALTER TABLE industrial_designs
    ADD COLUMN IF NOT EXISTS display_label TEXT;

-- ADR-0142 decision 4: a caller may still supply a bounded, optional
-- display label ("who to show in the UI"), stored separately from and
-- never substituted for the verified principal now recorded as the
-- authoritative actor/author (gaps.d/design-constitution-display-label-
-- not-stored-separately.md). Additive on industrial_design_materializations
-- only, applied by MaterializationStore -- see migration 0060's comment for
-- why this is a separate key from constitution_proposals' display_label
-- column rather than one migration bundling both ALTERs.
ALTER TABLE industrial_design_materializations
    ADD COLUMN IF NOT EXISTS display_label TEXT;

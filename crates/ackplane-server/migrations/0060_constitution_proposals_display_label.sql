-- ADR-0142 decision 4: a caller may still supply a bounded, optional
-- display label ("who to show in the UI"), stored separately from and
-- never substituted for the verified principal now recorded as the
-- authoritative actor/author (gaps.d/design-constitution-display-label-
-- not-stored-separately.md). Additive on constitution_proposals only --
-- the industrial_design_materializations half of this same decision is
-- migration 0061, applied by MaterializationStore, because bundling both
-- ALTERs into one migration key made ConstitutionStore's connect() touch a
-- table it does not own and cannot guarantee exists yet: on a genuinely
-- fresh database, ConstitutionStore::connect() can run before
-- MaterializationStore::connect() ever creates industrial_design_
-- materializations (migration 0032), and the shared long-lived dev
-- database masked this because that table already existed there from
-- unrelated prior activity. Splitting by owning store means neither
-- store's connect() ever depends on a table only the other one creates.
-- The industrial_designs/industrial_design_decisions half of the original
-- gap is deferred to a follow-up, since design_store.rs carried a live
-- ADR-0143 pool-migration claim at the time this lands.
ALTER TABLE constitution_proposals
    ADD COLUMN IF NOT EXISTS display_label TEXT;

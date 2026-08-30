- **Design/Constitution mutations drop a caller-supplied display label instead
  of storing it separately, narrowing ADR-0142 clause 4 — MEASURED
  2026-08-30, OPEN.** ADR-0142 decision 4 says a caller "may still supply a
  bounded, optional display label (e.g. 'who to show in the UI'), stored
  separately from and never substituted for the verified principal." This
  change stops trusting `proposed_by`/`actor`/`author` as identity (the
  authoritative value is now always the Bridge's verified principal) and
  keeps accepting those fields as optional input for backward wire
  compatibility, but does not persist them anywhere: `design_store`,
  `design_materialization_store`, and `constitution_store::proposals` have no
  column to hold a label distinct from the authoritative actor/author, so a
  caller-supplied value is silently discarded rather than "stored
  separately."
  The core safety property ADR-0142 exists for (attribution is un-forgeable)
  is intact either way -- this narrows only the secondary, explicitly
  optional half of clause 4.
  **What is actually needed:** a nullable `display_label`-shaped column on
  each of the three tables (`industrial_designs`/`industrial_design_decisions`
  wherever `record_decision` lands its row, `industrial_design_materializations`,
  `constitution_proposals`), a migration, and threading it through
  `CreateDesignRequest`/`RecordDecisionRequest`/`RecordMaterializationRequest`/
  `ProposeConstitutionClauseRequest` and their read responses -- separate,
  reviewable schema work rather than something to fold into ADR-0142's own
  slice quietly.

- **Design/Constitution mutations drop a caller-supplied display label instead
  of storing it separately, narrowing ADR-0142 clause 4 — MEASURED
  2026-08-30, NARROWED 2026-08-31: two of three tables closed, one open.**
  ADR-0142 decision 4 says a caller "may still supply a bounded, optional
  display label (e.g. 'who to show in the UI'), stored separately from and
  never substituted for the verified principal." This change stops trusting
  `proposed_by`/`actor`/`author` as identity (the authoritative value is now
  always the Bridge's verified principal) and keeps accepting those fields as
  optional input for backward wire compatibility, but does not persist them
  anywhere: `design_store`, `design_materialization_store`, and
  `constitution_store::proposals` have no column to hold a label distinct
  from the authoritative actor/author, so a caller-supplied value is
  silently discarded rather than "stored separately."
  The core safety property ADR-0142 exists for (attribution is un-forgeable)
  is intact either way -- this narrows only the secondary, explicitly
  optional half of clause 4.
  **Closed for `constitution_proposals` and `industrial_design_materializations`:**
  both tables gained a nullable `display_label` column (migration
  `0060_design_constitution_display_label.sql`), threaded through
  `ProposeConstitutionClauseRequest`/`ConstitutionProposal` and
  `RecordMaterializationRequest`/`MaterializationRevision`, and through
  Bridge's `ProposeClauseRequest`/`ConstitutionProposalResponse` and
  `RecordDesignMaterializationRequest`/`MaterializationRevisionResponse`.
  **Still open:** `industrial_designs`/`industrial_design_decisions`
  (`design_store.rs`, wherever `record_decision` lands its row) -- left out
  of this pass because `design_store.rs` carried a live ADR-0143
  pool-migration claim at the time this landed. What is actually needed there
  is unchanged: a nullable `display_label` column, a migration, and threading
  it through `CreateDesignRequest`/`RecordDecisionRequest` and their read
  responses, following the exact same shape the other two tables just used.

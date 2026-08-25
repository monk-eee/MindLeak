- **`ConstitutionStore` gains a Bridge-originated proposal ledger
  (ADR-0126).** `propose_clause`/`list_proposals`/`withdraw_proposal`
  (`constitution_store/proposals.rs`, table `constitution_proposals`)
  record a suggested constitution clause change, idempotent on
  `(tenant_id, repository_id, proposal_id)` the same way
  `record_publication` already is, gated to its own author for withdrawal.
  A proposal carries no authority: it is never read by
  `get_active`/`publish`, and it never causes a repository's constitution
  to change on its own — only that repository's own local
  `amend_constitution` flow still does that (ADR-0082 decision 4, ADR-0121
  decision 2, both unchanged by this). This is the storage slice only; a
  Bridge route and UI to reach it are follow-on work.

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
  decision 2, both unchanged by this).
- **The Bridge Constitution page (`/constitution`) is now editable, not
  just read-only.** Three new tenant-scoped routes reach the ledger above
  (`GET`/`POST /api/v1/repositories/:id/constitution/proposals`, `POST
  .../proposals/:proposal_id/withdraw`), and the page itself gained a
  "Propose a clause change" form plus a rendered list of pending/withdrawn
  proposals with a withdraw action. The published constitution endpoint
  itself (`/api/v1/repositories/:id/constitution`) stays read-only —
  every mutation this page can make targets the proposals sub-resource
  only, never the authoritative table.

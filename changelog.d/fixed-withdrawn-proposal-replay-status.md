- **Replaying a withdrawn constitution proposal no longer claims it is
  `proposed`.** `crates/ackplane-server/src/constitution_store/proposals.rs`
  treated any byte-identical replay of `propose_clause` as an idempotent
  no-op without checking the stored row's status, so the Bridge handler in
  `crates/ackplane-bridge/src/handlers/repository/constitution.rs` returned
  its hard-coded `status: "proposed"` response even after the proposal had
  been withdrawn, contradicting `list_constitution_proposals`, which still
  reported it as `withdrawn`. Withdrawal is this identity's one terminal
  mutation (ADR-0126), so `propose_clause` now refuses to replay a withdrawn
  proposal with a new `ConstitutionStoreError::ProposalWithdrawn` error, and
  the Bridge handler surfaces it as `410 Gone` instead of a false success.
  Verified against a live database via `ACKPLANE_TEST_DATABASE_URL` with new
  propose -> withdraw -> replay regressions at both the store and handler
  layers.

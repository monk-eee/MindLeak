- Added ADR-0142: the hardened loopback profile (ADR-0098's salt-derived
  tenant token) is also the verified principal ADR-0125 decision 2 requires
  for Bridge Work commands, and the accountable identity Design mutations
  and Constitution proposals record, under the identical self-hosted
  single-tenant scope ADR-0128 already established for privileged
  Administration. Reconciles a real contradiction between four ADRs accepted
  within three days of each other: ADR-0123's Design mutations and
  ADR-0126's Constitution proposals trust a caller-supplied identity string
  (ADR-0126's own words: "honest but weak"), while ADR-0125 permanently
  refuses every Work command under the same profile -- and ADR-0128, one day
  later, never revisited either. Design-only; an implementing agent still
  needs to thread the resolved verified principal through
  `work_command_api/`'s handlers and `design_api.rs`/`propose_clause`'s
  identity fields, and add the regression tests for both changes.

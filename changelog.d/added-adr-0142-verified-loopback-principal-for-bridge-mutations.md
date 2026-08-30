- **The Bridge's hardened loopback profile is now a verified principal for
  Work commands, Design mutations, and Constitution proposals (ADR-0142).**
  ADR-0128 already recognized the salted `development_tenant_token` as a real
  verified principal for Administration, for a self-hosted, single-tenant
  deployment; this extends the identical recognition to the three other
  mutation surfaces, closing a contradiction between four Bridge ADRs
  accepted within days of each other:
  - **Work commands execute.** `crates/ackplane-bridge/src/work_command_api/`
    built and merged under ADR-0125 were fully wired, fully tested, and
    permanently inert -- every one of the ten commands resolved to a typed
    `authorization_unavailable` refusal. `submit`/`confirm` now build a
    `WorkCommandAuthorization::Verified` principal (`principal_id`/`tenant_id`
    the salted token, scoped to the visible repository, the full closed
    command vocabulary, no adopted policy or delegation), so a well-formed,
    correctly-attributed request records a genuine `pending_confirmation`
    preview instead.
  - **Design and Constitution attribution is un-forgeable, not merely
    labeled.** `propose_design`, `record_design_decision`,
    `record_design_materialization`, `propose_constitution_clause`, and
    `withdraw_constitution_proposal` stop trusting a caller-supplied
    `proposed_by`/`actor`/`author` string as identity. The authoritative
    value is now always the Bridge's own verified principal; the request
    fields are still accepted (for backward wire compatibility) but are no
    longer persisted or trusted.
  - **Bug fix alongside:** `WorkCommandService`'s authorization check used to
    refuse *any* request whenever the verified principal's `policy_refs` was
    empty, even when the request itself named no policy either. That would
    have permanently blocked ADR-0142's own no-adopted-policy Work command
    principal (clause 5: Work commands do not gain an
    `AdministrationPolicy`-style policy layer) from submitting anything --
    the exact capability this ADR exists to unlock. Fixed to a plain equality
    check between what the principal carries and what the request names, with
    a regression test that fails against the unfixed check.
  - A non-loopback, multi-tenant deployment is unaffected: ADR-0094's refusal
    of a non-loopback bind without a production verifier remains the single
    enforcement point.

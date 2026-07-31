- **The Design Board no longer records an accept/reject the ADR file cannot
  carry.** Accepting a design called `design_decide` first and only then wrote
  the ADR's Status field, so when the file could not be resolved — the ADR is on
  `main` but absent from the open checkout — the ledger had already accepted
  while the file still said `Proposed`, and the reviewer saw only `cannot find
  docs/adr/…`. That drift is permanent (it is the fingerprint ADR-0072 carries).
  The board now resolves and stages the file write before recording the
  decision, so a decision that cannot be written is not recorded at all, and the
  failure names the checkout to open rather than only what was missing.

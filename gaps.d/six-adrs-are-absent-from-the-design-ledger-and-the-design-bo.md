- **Six ADRs are absent from the design ledger, and the Design Board reads empty
  as a result — MEASURED 2026-07-29, OPEN.** `design_board` returned `[]`.
  That is partly correct: it lists only *actionable* items — proposed ADRs
  awaiting a decision, and accepted designs awaiting promotion — and all 65
  registered items are `accepted` and `materialized`, so none is actionable.
  The rest is not correct. Measured against `origin/main`: **69 ADR files, 63
  registered**. `0063`, `0064`, `0066`, `0067`, `0068` and `0069` have no design
  item at all, so `reconcile_designs` has not run since they landed and they
  cannot appear on the board however the board is read.
  Syncing now would not simply fix it. Four of the six — `0063`, `0064`, `0067`,
  `0069` — declare `Status: Accepted` **in the file**, and per the renamed-ADR
  entry below an ADR merged with acceptance already written becomes another
  undecided row: it asserts a decision the ledger never recorded, so it arrives
  needing a decider it never had. `0066` and `0068` are `Proposed` and would
  sync cleanly.
  The convention that avoids this is already written down — author an ADR as
  `Status: Proposed`, accept it through the Design Board, and let the file
  follow the decision — and it is not being followed. **ADR-0064 is mine and I
  broke it**, writing `Status: Accepted` straight into the file. — Impact: the
  Design Board cannot be used to find undecided design, because the undecided
  ADRs are the ones missing from it. — Not fixed here: registering the six is a
  decision about four assertions of acceptance that nobody recorded, which is a
  maintainer's call, not a sweep.

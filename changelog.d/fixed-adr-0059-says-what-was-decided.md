- **ADR-0059 now says what the ledger decided nine hours earlier.**
  `design:0059-the-tool-surface-is-a-vocabulary` was accepted by `monk-eee` and
  materialized — it is what spawned the four tool-collapse tasks — while the ADR
  file still read `Status: Proposed`. Nothing reconciles those two directions:
  the Design Board sync reads files into the ledger, and no step writes a ledger
  decision back into the file.
  That gap stopped work, not just confused a reader. Surveying the board the
  same day, an agent read the file, concluded the four collapse tasks were
  implementing an undecided design, and declined to claim any of them. The file
  is now `Accepted` and names the decision that made it so.
  Known gaps also records what the sweep behind this found: **69 ADR files on
  main, 63 registered as design items.** `0063`, `0064`, `0066`, `0067`, `0068`
  and `0069` have no design item, which is why `design_board` reads empty — the
  undecided ADRs are precisely the ones missing from it. Four of those six
  assert `Status: Accepted` in the file without a recorded decision, so
  registering them is a maintainer's call rather than a sweep.

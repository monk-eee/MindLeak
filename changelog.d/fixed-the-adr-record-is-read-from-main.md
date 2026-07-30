- **The ADR record is read from main, not from whichever worktree asked.**
  `readAdrFiles` listed `docs/adr` on disk, so the design record it reported was
  whatever the asking checkout happened to hold. Under ADR-0038 that is a
  different subset in every worktree, while the design ledger it is compared
  against is one shared per-repository database — so `design-audit` manufactured
  drift that did not exist, reporting every ADR present on main but absent
  locally as a ledger row with no file. Measured across 84 attached worktrees:
  75 ADRs on `origin/main`, and the union across all 196 remote branches also
  75, so main is the complete record and nothing is ever branch-only. Yet 65 of
  those worktrees were missing between 1 and 26 ADRs, and the checkout the
  extension itself reads was 904 commits behind and held 49 of 75 — a third of
  the design record absent, with no error of any kind. `design-audit` now reads
  the record from `origin/main` and names the source in its output. It falls
  back to the working tree only when the ref cannot be resolved, as in a fresh
  clone with no remote, and says so when it does: falling back silently is the
  failure being fixed, because a partial record that reports itself as complete
  makes every tool downstream state confident nonsense. `adr-index` deliberately
  still reads the working tree — it generates the index for the commit being
  made, so a newly authored ADR must appear in it.

- **A renamed ADR leaves an unreachable Design Board row forever — OPEN.** —
  Observed Jul 2026 while investigating "the Design Board seems to have errors".
  `list_designs` returned two rows, `design:0036-one-work-surface` and
  `design:0037-one-work-surface`, whose `adr_path` matches no file on any branch:
  both are residue from renumbering the same ADR (0036 → 0037 → finally 0040).
  Every tree row is wired to `mindleak.design.openAdr`, so clicking either throws
  and surfaces an error toast — the reported symptom. The cause is that
  `reconcile_designs`
  ([`facade/design.rs`](crates/lodestar-core/src/facade/design.rs)) is
  upsert-only and keys on ADR path, so a rename registers a new id and orphans
  the old one; there is no `retire_design`. — Medium impact: no decision is lost
  and no state is wrong, but the board accumulates unclickable rows that erode
  trust in it. — Left for later, and deliberately **not** fixed by auto-retiring
  designs whose file is absent: under ADR-0038 several worktrees on different
  branches share one `spec.db`, so "file missing from this checkout" is a normal
  branch-local condition, and retiring on it would delete live decisions. The fix
  is an explicit, attributed `retire_design` plus a rule about whether
  branch-local ADRs should register at all — that wants an ADR.

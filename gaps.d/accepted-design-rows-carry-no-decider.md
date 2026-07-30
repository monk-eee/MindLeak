- **Accepted design rows carry no decider, so the ledger asserts decisions
  nobody made — MEASURED 2026-07-30, OPEN.** `design_query(view="ledger")`
  returns 72 rows, of which **6 are `accepted` with `decided_by` empty**:
  ADR-0063, 0064, 0067 and 0069 among them. An accepted row with no decider
  records that a decision happened while naming nobody who made it, which is
  exactly the shape ADR-0042 exists to prevent for retirement and ADR-0071 for
  task review.

  The cause is a convention that is written down and not followed: author an ADR
  as `Status: Proposed`, accept it through the Design Board, and let the file
  follow the decision. An ADR merged with `Status: Accepted` already written into
  it arrives at `reconcile_designs` asserting an acceptance the ledger never
  recorded, so it lands undecided and stays that way.

  This entry replaces the earlier "six ADRs are absent from the design ledger"
  measurement, which is no longer true — all six are registered and the ledger
  holds 72 rows. The prediction that fragment made about syncing them has simply
  come true instead: registering them produced rows asserting acceptance with no
  decider. Reported as its own gap rather than folded away, because the previous
  fragment covering undecided imported statuses
  (`imported-adr-statuses-landed-accepted-with-nobody-named.md`) was deleted
  during a catalog cleanup, leaving this measurable defect with nothing naming
  it.

  Not fixable by a sweep: assigning a decider to four assertions of acceptance
  that nobody recorded is a maintainer's call, not an automated backfill. What
  can be mechanised is refusing the shape at the door — either registering an
  `Accepted`-in-file ADR as `proposed` and requiring an explicit acceptance, or
  refusing to register it at all until the file says `Proposed`.

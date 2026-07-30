- **Accepted design rows carry no decider, so the ledger asserts decisions
  nobody made — MEASURED 2026-07-30, RE-MEASURED 2026-07-31, OPEN.** The ledger
  holds 76 accepted rows, of which **3 are `accepted` with `decided_by` empty**:
  `docs/adr/0036-one-work-surface.md`, `docs/adr/0037-one-work-surface.md` and
  `docs/adr/0074-coverage-is-a-prediction-until-conformance-speaks.md`. An
  accepted row with no decider records that a decision happened while naming
  nobody who made it, which is exactly the shape ADR-0042 exists to prevent for
  retirement and ADR-0071 for task review.

  — *The count fell and the finding did not, which is the part worth reading.*
  First measured as 6 of 72, naming ADR-0063, 0064, 0067 and 0069. All four now
  carry `decided_by=monk-eee`, so that backlog was worked. But ADR-0074 was
  registered undecided on 2026-07-30 — the same day the gap was written — so the
  population is not draining, it is being refilled at roughly the rate it is
  cleared. Read the intake, not the count: a backlog that is worked and refilled
  looks identical to one nobody has touched, and only the arrival dates tell
  them apart. The two older rows are a curiosity of their own: ADR-0036 and
  ADR-0037 were both registered on 2026-07-27 under the *same* title, "One Work
  surface with advanced proof", which reads like one decision filed twice rather
  than two decisions.

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

  Not fixable by a sweep: assigning a decider to assertions of acceptance that
  nobody recorded is a maintainer's call, not an automated backfill. What can be
  mechanised is refusing the shape at the door — either registering an
  `Accepted`-in-file ADR as `proposed` and requiring an explicit acceptance, or
  refusing to register it at all until the file says `Proposed`. The
  re-measurement is the argument for doing that rather than sweeping again:
  a sweep clears the rows and leaves the intake open.

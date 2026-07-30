- **A gap fragment can declare itself OPEN long after its defect is fixed, and
  no check can catch it — MEASURED 2026-07-30, OPEN by construction.**
  `scripts/gaps.mjs --check` refuses a fragment whose heading carries a terminal
  marker with no OPEN residual, which closes the direction where a fixed gap
  advertises itself as fixed and lingers. The opposite direction is the one that
  misleads, and it is invisible to the validator: a fragment that still says
  OPEN, VERIFIED or MEASURED while the thing it describes has been repaired. The
  validator reads a heading's self-declared status; whether that status is
  *true* lives in the code the fragment describes, so no rule over headings can
  decide it.

  Measured against `origin/main` on 2026-07-30, auditing the fragments that make
  a falsifiable claim about a named symbol, file or setting. Four of them were
  contradicted by the tree:
  `a-task-does-not-record-the-branch-it.md` ("`Task` has no branch field" —
  `model/executive.rs` declares `pub branch: Option<String>`, and a live claim
  reads it back);
  `a-renamed-adr-leaves-an-unreachable-design-board.md` ("there is no
  `retire_design`" — `facade/design.rs` has one under ADR-0042, and the ledger
  now reports zero rows whose `adr_path` is missing);
  `the-post-commit-ingest-hook-is-not-installed-so-commits-land.md` (the shared
  hooks directory now holds `post-commit`);
  and `six-adrs-are-absent-from-the-design-ledger-and-the-design-bo.md` (all six
  are registered; the ledger holds 72 rows). The first two were removed, the
  last two narrowed onto the residual that survived.

  The cost is not tidiness. An agent with an empty board reads this catalog to
  find work. On 2026-07-30 that agent picked a fragment, created a task, claimed
  it, and only then discovered the work had already shipped — one claim and one
  task creation spent on a fixed defect. Sampling was not unlucky: the first two
  fragments opened were both false.

  Not every fragment is stale, and the audit proved that too rather than
  assuming it: `squash-merging-is-still-enabled-at-the-button.md` was checked the
  same way and holds — the repository still reports `allow_squash_merge: true`.

  No fix is proposed, because the obvious one is wrong. A status vocabulary rule
  would only ever check that a fragment agrees with itself. What the class needs
  is periodic re-verification against the tree — cheap for fragments naming a
  symbol or a setting, and not mechanisable for the rest — so it is recorded
  here as a standing maintenance cost rather than a defect awaiting a patch.

- **New silent knowledge is guarded; 63 existing records remain unreadable —
  MEASURED, PARTLY FIXED, OPEN.** The conformance advisory matches recorded knowledge on
  referenced nodes and nothing else, so a record whose evidence carries no
  `nodes` array is stored, counted, decayed, and structurally incapable of
  reaching any agent. Measured 2026-07-30 with `active_knowledge`: **63 of 149
  active records report `surfaces: false`**. They are not marginal notes. They
  are among the most expensive lessons in the ledger, and the cost of their
  silence was paid again the same day, by an agent that had no way to know they
  existed:

  - `knowledge:829fcb369525` — "testing a facade method proves the logic and
    says nothing about the wiring", recorded because `merge_evidence` shipped
    broken for every caller when its name was never added to `requires_session`.
    That is the identical defect re-diagnosed from scratch in PR #200, and the
    identical class re-diagnosed again in PR #205.
  - `knowledge:462bd4f40029` — a guard is only as good as the name it names,
    recorded when a test asserted over tool names ADR-0059 had retired. The same
    staleness was rediscovered in PR #205 and the guard rewritten in PR #219.
  - `knowledge:b482f79e1f4a` — a deprecated tool name silently loses its
    argument validation. The same class, found a third time.
  - `knowledge:fcdebef12c49` — "I skipped the mandatory `advise` pre-flight and
    paid the exact price it exists to prevent." Re-proved today at the cost of
    four task closures landing `drift` or `needs_human`, every one of which now
    needs a human to resolve.
  - `knowledge:e5e56544342e` — the running MCP server can be an hour behind the
    source and reports confidently anyway. Cost most of a session before the
    binaries were redeployed.

  The write path is fixed: `record_knowledge` now reports `surfaces` and, when
  false, names the missing field while the caller still has the node ids to hand
  (PR #230). That stops the population growing. It does nothing for the 63
  already there, and there is no verb that repairs one — knowledge is
  append-only, `reconfirm_knowledge` only refreshes the clock, and
  `prune_knowledge` prunes by decay rather than by id. So today the only repair
  is to re-record a record's content with proper `nodes`, which requires
  re-verifying that it is still true first; several of these describe code that
  has since changed, and copying a stale claim forward would be worse than
  leaving it silent.

  The uncomfortable reading is the one worth keeping. This is not a backlog of
  missing notes. The repository had already learned nearly everything that was
  rediscovered at length on 2026-07-30 — the wiring-versus-facade gap, the guard
  naming retired tools, the deprecation that disables validation, the stale
  binary, the skipped pre-flight — and the mechanism built to carry those
  lessons forward could not deliver a single one of them. A memory that cannot
  be read is not a memory; it is a cost centre that produces the feeling of
  having learned.

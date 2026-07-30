- **The Design Board shows what needs a decision, and asks once.**
  It read `design_query view=ledger` — the durable record, including every
  historical and materialized item — and then issued one further
  `view=promotion` call per materialized design to decorate rows nobody is being
  asked to act on. Measured against the live ledger: one refresh rendered 75
  rows of which 5 were actionable, at **70 MCP calls**, 69 of them fetching
  detail for finished work. The refresh is wired to a file watcher over
  `docs/adr`, so every ADR touch paid it again — which is most of why the server
  felt slow while the board felt cluttered. `design_query` already named the
  right question: its own description defines `view=board` as "actionable items:
  proposed ADRs awaiting a human decision plus accepted designs awaiting or
  retrying promotion", which is the board this view is named after. The board
  now reads that view: **1 MCP call, 5 rows**. The promotion fan-out is
  unchanged in kind but bounded by what is actually shown, and a test pins both
  the view and the call count, because a fan-out is invisible from the UI and
  would come back unnoticed. The durable record is still `view: "ledger"` for
  anything auditing it.

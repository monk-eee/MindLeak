- **An accepted ratchet baseline records why, not only who.** `RatchetBaseline`
  carried `value`, `reviewed_by` and `reviewed_at`, and
  `with_reviewed_baseline` refused an empty reviewer but took no reason at all.
  Its own doc comment cites SPEC-CONSTITUTION §4 — "whether the baseline was
  trustworthy" is among the questions a ratchet cannot answer about itself, "so
  the answer has to arrive from outside the mechanism" — but a signature is not
  that answer. Knowing who moved the number says nothing about whether moving it
  was justified, so the mechanism recorded the attribution and discarded the
  judgement it exists to import.
  Measured 2026-08-14: `control:rust-module-length` went from 7 to 8 to absorb
  an 833-line module, attributed to a session and explained nowhere, because
  there was nowhere to explain it.
  `accept_ratchet_baseline` now requires a `reason` on the facade and the MCP
  tool, and refuses a blank one exactly as it already refused a blank reviewer.
  The field is optional in storage only so that baselines accepted before it
  existed read back as `None` rather than failing the control shut — a control
  that cannot be read is a control that cannot refuse. No reason is invented for
  them.

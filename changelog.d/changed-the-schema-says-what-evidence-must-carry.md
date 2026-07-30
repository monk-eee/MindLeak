- **`record_knowledge` now says what evidence must carry, before the record is
  written.** The conformance advisory matches recorded knowledge on referenced
  nodes and nothing else, so evidence without a `nodes` array produces a record
  that is stored, counted, decayed on schedule, and can reach nobody. The
  schema described that field as "JSON provenance" — accurate, and silent about
  the one thing that decides whether the lesson ever arrives. It now names the
  `nodes` array, shows the shape, and states the consequence of omitting it.
  This is where the caller decides what to send; the `surfaces` warning added
  in the reply is the backstop for getting it wrong anyway, and it necessarily
  arrives after the record already exists. Measured when this landed: 67 of 170
  active records, 39%, name no nodes. Among them are the lessons most worth
  having — that skipping the ADR-0029 pre-flight causes the drift verdicts
  people then blame on goal bindings, and that testing a facade method proves
  the logic and says nothing about the MCP wiring, which is exactly how
  `merge_evidence` shipped refusing every caller. Both were written so the next
  agent would not repeat the mistake, and neither could be delivered to anyone.
  `consolidate` was never affected: it requires `evidence_node_ids` outright,
  which is why only the free-form path grew a backlog. A test pins the
  description so the guidance cannot quietly regress to something true and
  useless.

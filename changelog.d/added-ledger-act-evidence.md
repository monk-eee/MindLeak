### Added
- New MCP tool `ledger_act_evidence` (ADR-0110): builds conformance evidence
  from one Lodestar-internal ledger act — a design registration, a design
  decision, a waiver grant, or a constitution amendment — instead of refusing
  a completion outright because the act touched no MindLeak node. The lookup
  is the verification itself, entirely inside Lodestar: it refuses when the
  recorded actor does not match the calling agent, when the act predates the
  claim, or when the task is not held by the caller. The base conformance gate
  now also treats a non-empty `ledger_act_ids` the same as a changed MindLeak
  node, so evidence built this way can reach a real verdict instead of the
  "no provenance-bearing mutation" refusal, and a task receipt built entirely
  from a verified ledger act can affirm.

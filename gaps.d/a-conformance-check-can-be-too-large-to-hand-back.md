- **A conformance check can be too large to hand back, so `complete` becomes
  unreachable.** `check_conformance` returned record 551 carrying 50 findings
  totalling 54,756 characters; only two were verdict-bearing, and the other 48
  were `advisory: learned knowledge ...` entries surfaced for the 16 changed
  nodes. `Lodestar::complete_task` in
  [`crates/lodestar-core/src/facade/conformance/mod.rs`](../crates/lodestar-core/src/facade/conformance/mod.rs)
  compares `check.findings.join("; ")` against the stored record and derives the
  conformance token from the same value, so a caller must return every finding
  verbatim even though the server already holds the record it is being asked to
  echo. There is no by-`id`-and-`token` path. An MCP client with a bounded
  argument payload therefore cannot complete a task whose changed nodes carry a
  lot of learned knowledge, and the advisory volume scales with the size of the
  knowledge base rather than with the work being judged. Observed while
  completing `task:fc192f8ed4bf` (two commits, 16 changed nodes). Impact is
  confined to the lifecycle transition: the check itself is correct, the record
  is durable, and the ledger cannot be corrupted by a wrong echo because a
  mismatched findings list is rejected rather than accepted. Left for later; the
  likely fix is to accept the check by `id` and `token` alone, or to keep
  advisory knowledge out of the tamper-evident findings that seed the token.

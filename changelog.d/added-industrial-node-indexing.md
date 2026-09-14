- **Explicit Industrial indexing:** `ackplane-mcp` now offers `index`, which
  embeds projected labels on the enrolled node and publishes through its
  credential-owning companion. Bounded batches, source-change refusal and
  partial-progress results avoid false completion. Empty queues make no model
  call; redirects, oversized responses and malformed vectors are refused.
  The shared response parser replaces the two local copies and now rejects
  malformed explicit indices instead of attaching vectors by response order.
  Ackplane still performs no inference; shared recall and freshness reporting
  remain unfinished.

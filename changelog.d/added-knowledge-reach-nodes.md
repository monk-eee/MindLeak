### Added

- **Knowledge names the nodes it reaches, not just how many.** `KnowledgeStore`
  has always carried `reach_node_ids`, and the gRPC KnowledgeService has always
  returned them, but the Bridge reduced them to `reach_count:
  entry.reach_node_ids.len()` — so an operator was told a lesson reached
  "26 nodes" and given no way to see or act on which twenty-six. Both the
  history and revalidation-queue responses now carry a bounded
  `reach_node_ids` preview alongside `reach_count` and a `reach_truncated`
  flag, following the same shape `active_knowledge` already uses: the count
  keeps describing the whole set, and only the array is capped.

  The Knowledge page renders those nodes and opens them in the Context Graph,
  so a recorded lesson can be read against the code it governs. A record that
  reaches nothing offers no link — seeding a traversal from an empty set asks
  the graph a question with no subject.

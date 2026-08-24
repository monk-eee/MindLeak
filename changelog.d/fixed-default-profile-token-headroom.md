- **Fixed**: restored real headroom under the default MCP tool profile's
  ADR-0093 token budget (5,500 tokens) by trimming narrative from
  `task_claim`, `task_transition`, `task_query`, and `active_knowledge`'s
  descriptions that already duplicated their own parameter-level
  descriptions, or (for `active_knowledge`) was pure historical backstory
  rather than usage guidance. No behavior change. The default profile was
  sitting at ~5,540 of 5,500 tokens with the next legitimate schema addition
  (`task_query`'s `detail` parameter) applied on top, breaching the headroom
  bound `tools::tests::the_default_profile_is_under_budget` enforces; it now
  sits at ~5,263 tokens, comfortably under both the headroom bound and the
  ADR-0059 hard ceiling (6,000 tokens).

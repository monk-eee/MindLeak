- **Task resolution now says what its reviewer field actually proves
  (ADR-0071).** `resolve_task` records a non-empty reviewer label in
  `resolved_by`; the label is attributable but not authenticated. Lodestar has
  no human identity provider, so core errors/docs and the MCP contract no longer
  call the value a verified identity. The same-string self-review guard remains,
  but any other label is accepted and stored unchanged. A regression pins that
  behavior with a deliberately non-credential label so the API cannot quietly
  regain stronger wording than its mechanism supports.

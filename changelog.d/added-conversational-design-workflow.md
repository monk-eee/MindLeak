- Added the `ackplane-mcp` `design_workflow` prompt and loopback operator CLI
  design authoring: bounded offline proposal preview, digest-confirmed
  publication and lifecycle decisions, list/detail reads, and explicitly
  confirmed links to existing Work and constitution records. Copilot can draft
  and persist requirements without using the Bridge web form; model selection
  and execution authorization remain separate.
- Work submission can use Bridge's verified operator principal without asking
  the caller to supply its internal identity. Explicit forged identities remain
  refused. Fixed false materialization retry conflicts from unordered task
  references; writes and reads now use stable canonical task ordering.

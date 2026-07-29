- **One shared stdio MCP server gave concurrent chat sessions the same agent id — FIXED.** —
  The first ADR-0030 implementation qualified identity with a *per-process*
  nonce in both MCP entry points, but VS Code multiplexes multiple concurrent
  chat sessions through a **single**
  long-lived MCP server process. All of those sessions therefore share one nonce
  and one identity (observed: `copilot-4e151e90` held three simultaneous live
  claims across independent sessions), so per-agent claim ownership, leases, and
  evidence attribution cannot distinguish the sessions. — Medium impact: owner
  guards and evidence loops treat distinct concurrent agents as one; there is no
  data loss, but coordination invariants degrade under real fleet use. — Fixed by
  ADR-0030 session registration: clients mint one token, both planes derive one
  stable identity, and every identity-bearing call is bound to that registered
  token rather than process state. The pinned Extension Host release smoke now
  asserts the session-qualified identity and current session-only task actions,
  rather than the removed process nonce/arbitrary allocation contract.

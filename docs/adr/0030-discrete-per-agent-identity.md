# ADR-0030: Discrete per-agent identity for concurrent coordination

- Status: Accepted
- Date: 2026-07-24
- Deciders: MindLeak maintainers
- Related: [ADR-0003](0003-agent-attribution-as-observed-edges.md) (attribution as
  `observed` edges), [ADR-0004](0004-intent-plane-spec-brain.md) (intent plane),
  [ADR-0015](0015-advisory-symbol-leases.md) (advisory leases / progressive
  handoffs), [ADR-0018](0018-conflict-safe-concurrent-editing.md) (shared-tree
  concurrency), [ADR-0024](0024-preflight-overlap-detection.md) (pre-flight
  overlap detection), [SPEC-INTENT.md](../SPEC-INTENT.md)

## Context

Both planes are built on a **per-agent identity**. Lodestar's atomic claim/lease
coordination ([ADR-0004](0004-intent-plane-spec-brain.md)) and MindLeak's
attribution `observed` edges + `list_agents` ([ADR-0003](0003-agent-attribution-as-observed-edges.md))
are keyed entirely on the agent id. `claim_task` is a compare-and-swap on that id;
`renew_lease`, the `complete_task` owner guard, `working_set`, and the agent roster
all trust it to tell one agent from another.

But the id is resolved **verbatim from a static environment variable** — `LODESTAR_AGENT`
in [`lodestar-mcp/src/main.rs`](../../crates/lodestar-mcp/src/main.rs) and
`MINDLEAK_AGENT` in [`mindleak-mcp/src/main.rs`](../../crates/mindleak-mcp/src/main.rs) —
and the installer writes a single hardcoded value (`--agent`, default `copilot`)
into `.vscode/mcp.json`. Nothing makes it unique per process.

**Consequently every agent launched from that workspace config shares one identity
(`copilot`).** Observed directly this session: two agents ran concurrently, both as
`copilot`; `board` showed `owner: copilot` on a task neither could claim as its own,
and each could renew or complete the other's claims. This is not a cosmetic issue —
it silently voids the exact guarantee the intent plane exists to provide:

- `claim_task`'s CAS does not *race* two same-id processes, it **aliases** them —
  both believe they own the task.
- `renew_lease` / `complete_task` owner guards pass for the wrong process.
- `working_set` and attribution merge unrelated sessions into one identity, and
  `list_agents` cannot see that more than one agent is at work.

Multi-agent coordination is the reason this project exists; a shared static id
turns N coordinating agents into one amnesiac identity.

## Decision

Identity is **per logical client session**, not per server process. This amends
the original process-nonce design: VS Code can multiplex concurrent chats through
one long-lived stdio process, so a process nonce still aliases those chats.

1. A client mints one opaque 128-bit lowercase-hex `session_id` and calls
  `open_session(session_id)` on both planes.
2. Each server validates and registers the token in process memory, then derives
  `session:v1:<base>:<first 16 SHA-256 bytes as 32 lowercase hex characters>`.
  `MINDLEAK_AGENT` and
  `LODESTAR_AGENT` are human-readable base labels only; they default to `agent`.
  Raw tokens are never persisted or logged.
3. Every identity-bearing call carries the registered `session_id`. The server
  resolves it and overwrites internal owner/evidence arguments, so an arbitrary
  per-call `agent` value cannot impersonate another session. Unknown or malformed
  tokens fail before domain dispatch.
4. The same token/base yields the same identity after a server restart and across
  both planes. Two tokens multiplexed through one process yield distinct owners,
  working sets, and evidence.
5. `recover_claim` is the only path from an expired legacy `<base>` or
  `<base>-<8hex>` owner into a registered session. It requires the exact old
  owner and a reason, refuses live/grace-period claims, wrong bases, and
  session-qualified siblings, starts a fresh evidence window, preserves scope
  and Q&A, and appends the full prior claim state to `task_claim_transfers`.
  Ordinary `claim_task` cannot bypass that audit path.

The VS Code extension mints one token per activation, registers it with both
children, verifies both return the same identity, and injects it after caller
arguments. Headless clients do the same explicitly. The old `*_AGENT_ID` pins and
process nonce are removed rather than retained as a second identity model.

## Consequences

- claim/lease atomicity, the `complete_task` owner guard, `working_set`, and
  attribution become **correct under concurrency** — the intent plane finally
  delivers its core anti-collision guarantee instead of aliasing.
- Distinct ids are a **prerequisite** for pre-flight overlap detection
  ([ADR-0024](0024-preflight-overlap-detection.md)) to attribute a working-set
  footprint to the right agent; that ADR should assume this one.
- Agent nodes multiply (one per session) — durable-by-design roster growth
  ([ADR-0021](0021-node-lifecycle-and-reaping.md)); active attention still decays,
  so the *active* roster stays meaningful. An optional "active since" filter on
  `list_agents` is a possible follow-up, not required here.
- Restarted clients retain identity only by retaining their session token; claims
  never transfer merely because a process restarted.
- Recovery is explicit, attributed, and append-only rather than an owner rewrite.
- Tests cover registry continuity, multiplexed owners/evidence, unknown tokens,
  schema removal of caller-selected ids, and guarded legacy recovery.

### Rejected alternatives

- **Installer-generated per-workspace id.** Two agents in the *same* workspace
  still share it. Insufficient — the collision is per process, not per workspace.
- **Per-process nonce.** Insufficient when one process multiplexes several chats;
  this was implemented and then falsified in production.
- **Unregistered per-call id.** Lets callers accidentally change identity or
  impersonate another session. Registration makes continuity explicit.
- **Persist raw session tokens.** Unnecessary credential-like state; deterministic
  fingerprints are sufficient for restart continuity.
- **Identity solely from the MCP `initialize` `clientInfo`.** A useful signal that
  could seed the base label, but clients may report identical `name`; an opaque
  client-session token is still required for uniqueness. May inform the base in
  a follow-up.

Implementation is shared by both MCP transports through `mindleak-session`, with
session-bound graph attribution, Lodestar ownership, guarded claim recovery, and
the optional VS Code product shell using the same protocol as headless clients.

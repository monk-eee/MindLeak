# ADR-0030: Discrete per-agent identity for concurrent coordination

- Status: Proposed
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

The **runtime agent identity is unique per server process**, while staying
human-traceable to a configured base label.

### Resolution order (each server, at startup)

1. **Explicit pin wins.** If a fully-qualified id is provided
   (`LODESTAR_AGENT_ID` / `MINDLEAK_AGENT_ID`), use it verbatim — for tests,
   single-agent setups, and deliberately fixed identities.
2. **Otherwise derive `"<base>-<nonce>"`.** Treat `LODESTAR_AGENT` / `MINDLEAK_AGENT`
   as a **base label** and append a short process-unique `<nonce>` (8 hex chars from
   a CSPRNG). With no base configured, the base defaults to `agent`.
3. **Log the resolved id once** at startup (stderr) so it is visible and greppable
   (`[lodestar-mcp] agent = copilot-3f9a1c02`).

### Installer stops pinning a shared id

The release installer ([`install.mjs`](../../editors/vscode/scripts/install.mjs))
writes only a **base label** (or omits the variable); it never writes a concrete id
that two concurrent processes would share. Explicit pinning stays available for
single-agent or reproducible setups via the `*_AGENT_ID` escape hatch.

### Identity is per session, not per logical human

A reconnect spawns a new process and therefore a new id. That is **correct** for
coordination: a dead session's claims expire and become reclaimable under the
existing lease semantics ([ADR-0015](0015-advisory-symbol-leases.md)) rather than
being silently inherited by an unrelated process that happened to reuse the same
static id. `list_agents` then shows the true set of live sessions
(`copilot-3f9a…`, `copilot-b2c1…`); attention still fades via `observed`-edge decay
([ADR-0003](0003-agent-attribution-as-observed-edges.md)), and a retired session's
agent node is retained but drops from the active roster
([ADR-0021](0021-node-lifecycle-and-reaping.md)).

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
- Existing pinned single-agent workflows are unaffected via the explicit-id path.
- New tests: a pure resolution test (explicit pin honoured; base → `base-nonce`;
  two resolutions differ) and an integration test that two engines with distinct
  ids cannot both win one claim, whereas a shared id aliases.

### Rejected alternatives

- **Installer-generated per-workspace id.** Two agents in the *same* workspace
  still share it. Insufficient — the collision is per process, not per workspace.
- **Hard-require an explicit unique id (error when missing).** Brittle for the
  common "just run it" path and pushes per-agent bookkeeping onto users. Keep the
  auto-unique default; keep explicit pinning optional.
- **Per-logical-agent stable id across reconnects.** Needs a durable client-identity
  handshake and reintroduces aliasing when two clients assert the same logical id.
  Session-scoped uniqueness is simpler and correct for coordination.
- **Identity solely from the MCP `initialize` `clientInfo`.** A useful signal that
  could seed the base label, but clients may report identical `name`; a nonce is
  still required for uniqueness. May inform the base in a follow-up.

This ADR carries no behavioural code; it is the design-first predecessor for the
implementation task. Implementation touches
`crates/{lodestar,mindleak}-mcp/src/main.rs` (resolution + a small shared nonce
helper), the installer (stop pinning a shared id), and the config reference in
[USAGE.md](../USAGE.md) / [README.md](../../README.md), plus the tests above.

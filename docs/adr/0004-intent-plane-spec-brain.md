# ADR-0004 — Intent Plane: a durable "spec brain" separate from the decay graph

- **Status:** Accepted
- **Date:** 2026-07-22

## Context

MindLeak is **episodic** memory: descriptive, decaying, zero-token. Even its
`intent` nodes are *post-hoc* (a commit rationalising what already happened, a
point-in-time `record_architectural_decision`), and they **decay** — a decision's
`relates_to` links have a 48h half-life and fall below the prune threshold in
about nine days. [ADR-0003](0003-agent-attribution-as-observed-edges.md) added
per-agent *attention* as decay-weighted `observed` edges, and states the property
plainly: "who is working here *now* naturally fades to who worked here once."

That fade is correct for memory and **wrong for coordination**. When several
agents broker work in parallel across worktrees of one repo, intent gets diluted
because each agent holds a local, drifting copy of it, and two agents can claim
the same work. The missing layer is a *shared, authoritative* source of design
intent plus a substrate to allocate and reconcile parallel work — the opposite of
a fading edge.

The missing layer has **inverted invariants** to everything MindLeak is built on:

| Axis | MindLeak (memory) | Intent Plane |
|---|---|---|
| Direction | backward (what happened) | forward (what we intend) |
| Lifetime | **decays on purpose** | **must not decay** |
| Write path | ingested telemetry (0-token) | deliberately authored |
| Consistency | local, lossy, tolerant | shared, authoritative, consistent |

Cramming durable spec/task state into the decay graph — new node types with a
huge half-life, or a background job that keeps intent alive — would violate
invariants 2 (decay is the point) and 3 (weight is derived, never stored). That
is the expedient hack this ADR exists to forbid.

## Decision

Introduce a **second plane** — the **Intent Plane** (working name **Lodestar**) —
as a *separate subsystem* (its own crate and store), not new node types on the
decay graph. Full design: [SPEC-INTENT.md](../SPEC-INTENT.md).

- **Three concerns, one plane:** a durable, versioned **Constitution** (goals,
  constraints, invariants), an **Executive** task ledger (allocation, claim,
  completion), and a **Conformance** check (drift/violation against active
  constraints).
- **Local, one machine.** The substrate is a single shared SQLite file (WAL) at a
  stable per-repo path, so every worktree of the same repo addresses **one**
  Intent Plane. No network listener, no auth — the same boundary as MindLeak
  ([SPEC.md §8](../SPEC.md)).
- **Coordination is a compare-and-swap, not a fading edge.** Claiming a task is a
  guarded `UPDATE … WHERE status='open' OR lease_expired`; the winner is the one
  transaction that mutates the row. Leases carry a TTL so a dead agent's claim is
  reclaimable. This is what keeps parallel agents from colliding.
- **The Constitution does not decay.** It is versioned; intent changes only
  through an explicit, attributed supersede — never through silent drift. It is
  exportable to a committed, human-reviewable file. Task/lease state stays local
  and gitignored (ephemeral coordination, regenerable).
- **The seam to MindLeak is loose.** Lodestar references MindLeak node ids
  (`artifact:…`, `symbol:…`) as opaque strings; the two share **no tables**.
  MindLeak becomes the Intent Plane's feedback sensor — actual-vs-intended.
- **Optional local SLM**, same posture as `consolidate.rs`: used only for goal
  decomposition and *semantic* conformance, off any hot path, degrading cleanly
  when no model is reachable. Deterministic checks always run regardless.

## Consequences

- **Do not** store durable spec/task state in the decay graph, and **do not**
  give nodes long half-lives to fake permanence — use the Intent Plane.
- **Do not** model a task claim as an `observed`-style edge; attention fades,
  a claim must not. Claims are guarded transactional writes.
- The Constitution is the source of truth every agent reads **before** acting;
  agents **claim** before working and report completion for conformance.
- Two MCP servers to start (memory + intent); they may be co-hosted later. Agents
  can run either or both.
- **Cross-repo** sharing stays unsupported (path-id collision, per ADR-0003).
  **Multi-machine** coordination (a networked intent server with auth) is
  explicitly out of scope and would need its own ADR.
- New surface demands new tests: the claim compare-and-swap under concurrency
  (two claimers, exactly one wins), lease expiry and reclaim, and the
  supersede-version chain.
- Provisional name **Lodestar** (alt: **Canon**) is cheap to change; it lives
  only in docs until code lands.

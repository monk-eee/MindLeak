# ADR-0024: Pre-flight work-overlap detection across both planes

- Status: Accepted
- Date: 2026-07-23
- Deciders: MindLeak maintainers
- Related: [ADR-0003](0003-agent-attribution-as-observed-edges.md) (agent
  attribution), [ADR-0004](0004-intent-plane-spec-brain.md) (loose seam),
  [ADR-0015](0015-advisory-symbol-leases.md) (no symbol locks),
  [ADR-0018](0018-conflict-safe-concurrent-editing.md) (worktree isolation),
  [SPEC-INTENT.md](../SPEC-INTENT.md), [EVALUATION.md](../EVALUATION.md)

## Context

Lodestar's coordination primitive is a compare-and-swap on a single task row: it
stops two agents claiming the **same task**, but nothing stops two agents
independently touching the **same files/symbols under different tasks**, or
**solving the same problem** in parallel. [EVALUATION.md](../EVALUATION.md) names
this as an untested gap: *"the two-agent duplicate-work scenario remains required
before general claims."*

This session produced live evidence that the gap is real, not theoretical:

- Two agents began splitting the same module (`graph.rs`, then `store.rs`,
  `tools.rs`) concurrently; one agent's `git mv` briefly moved another's modified
  file.
- A shared-index commit silently **deleted** another agent's committed ADR-0018
  because the index held a staged deletion the committer never made.
- A decay experiment's aggregate counts were swamped by other agents ingesting at
  the same time — parallel work on overlapping graph regions with no mutual
  awareness.

[ADR-0018](0018-conflict-safe-concurrent-editing.md) addresses the **physical**
stomp with scoped commit discipline and optional isolated validation/worktrees.
Those safeguards do not stop two agents from *choosing* overlapping work and
then colliding at merge/integration time, nor from duplicating effort. The missing piece is
**awareness before work starts**: *"given the paths/symbols I intend to touch, is
anyone else already here or already solving this?"*

## Decision

Add a **read-only, advisory, decay-aware pre-flight overlap check** that fuses
both planes. It complements — does not replace — worktree isolation
([ADR-0018](0018-conflict-safe-concurrent-editing.md)): worktrees prevent the
stomp, overlap detection prevents the wasted collision.

### Optional scope declaration on a claim

A claim may declare the paths (globs) and/or symbol ids it intends to touch.
Reuse/align with [ADR-0018](0018-conflict-safe-concurrent-editing.md)'s
path-ownership advisory rather than inventing a parallel mechanism — one
scope-declaration model, surfaced on the board as a planning hint.

### The checks: `check_overlap(paths[], symbols[])`

Two read-only tools preserve the loose plane boundary and are combined by the
caller:

1. **Active claims** whose declared scope intersects the requested paths/symbols.
2. **Recent agent footprint** from MindLeak — other agents' above-threshold
   `observed`/`modified` edges ([ADR-0003](0003-agent-attribution-as-observed-edges.md))
   on the same artifact/symbol nodes. **Decay-aware**: stale attention has faded
   below threshold and does not raise a false alarm, so only *currently hot*
   overlap surfaces.

### Advisory, never enforcing

Default **advisory** — the check *warns*, it does not lock, consistent with
[ADR-0015](0015-advisory-symbol-leases.md)'s refusal to fake a mutex the plane
cannot enforce (it does not own the filesystem). On a warning an agent should
coordinate, pick different work, or convert to a `blocked_by` handoff (the
supported same-file serialization). It is never a hard gate that can be bypassed
to grant false safety.

### Loose seam

Lodestar references MindLeak node ids as opaque strings; the check crosses the
seam by node id only, with no shared tables or transactions
([ADR-0004](0004-intent-plane-spec-brain.md)).

### Shipped surfaces

- Lodestar `claim_task` accepts optional path globs and opaque symbol ids and
  stores them only when the guarded claim succeeds. `task_scope` reads that
  declaration; `board` includes it as a planning hint.
- Lodestar `check_overlap` compares concrete requested paths and exact symbol ids
  with live, unexpired claim scopes. MindLeak `check_overlap` derives other
  agents' active direct or mutation-linked footprint using effective weight at
  query time.
- The VS Code allocator collects optional concrete paths/symbols, calls both
  tools before claiming, and displays a modal advisory. A user may coordinate,
  cancel, or explicitly claim anyway; no lock is acquired.

### Rejected alternatives

- **Hard filesystem locks / a write-coordinator.** Already rejected by
  [ADR-0015](0015-advisory-symbol-leases.md)/[ADR-0018](0018-conflict-safe-concurrent-editing.md);
  turns a stdio plane into a merge engine and grants false safety.
- **Per-symbol advisory leases.** [ADR-0015](0015-advisory-symbol-leases.md)
  rejected these as a mutex-that-isn't; a decay-aware *query* is honest about
  being advisory.
- **Polling for awareness.** Wasteful and racy; a pre-flight read at claim/start
  time is sufficient and cheap.
- **Ignoring MindLeak footprint (claims only).** Misses two agents editing the
  same file under unrelated tasks — exactly this session's collisions.

## Consequences

- A read-only `check_overlap` surface in each plane and an
  optional claim scope-declaration, coordinated with
  [ADR-0018](0018-conflict-safe-concurrent-editing.md) so scope declaration is
  shared, not forked.
- The check is decay-aware and read-only — no stored locks, effective weight never
  stored, deterministic hot path untouched, no network listener.
- The **two-agent duplicate-work benchmark** now passes: without pre-flight both
  tasks claim the same path; with it, both the live claim and MindLeak footprint
  surface, a 336-hour-old footprint stays absent, and B converts to a
  `blocked_by` handoff. The read leaves task state and graph counts unchanged.
- The benchmark proves the deterministic mechanism, not that independent agents
  always obey an advisory. Real-agent adherence remains an external evaluation.
- This accepted ADR coordinates with
  [ADR-0018](0018-conflict-safe-concurrent-editing.md) (physical integration
  discipline) and the progressive-handoff pattern
  ([ADR-0015](0015-advisory-symbol-leases.md)).

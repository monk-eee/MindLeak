# ADR-0015: Progressive task handoffs before advisory symbol leases

- Status: Accepted (no symbol-lease primitive)
- Date: 2026-07-23
- Deciders: MindLeak maintainers
- Related: [ADR-0004](0004-intent-plane-spec-brain.md) (intent plane),
  [SPEC-INTENT.md](../SPEC-INTENT.md)

## Context

Lodestar coordinates parallel agents through **task ownership**. `claim_task(id,
agent, lease_secs)` sets `owner` + `lease_expires_at` on a *task* row (`store.rs`)
via an atomic compare-and-swap; `renew_lease` extends it; expiry frees it for
another agent. The TTL is **already caller-supplied**, so the "heartbeat" pattern
(short lease + periodic `renew_lease`, auto-reclaim on crash) is expressible
**today** with no new primitive — it is a usage pattern, not a missing feature.

What is *not* modeled is locking a **code resource**. The proposal under
discussion: let agents lease individual symbols so they can partition work inside
one file — Agent A holds `symbol:src/lib.rs:RouterStruct`, Agent B holds
`symbol:src/lib.rs:HelperFn`, and both edit `src/lib.rs` "concurrently."

### The trap this ADR exists to name

**Lodestar does not own the filesystem and performs no merges.** File writes are
line/byte-oriented; two edits to *different AST nodes in the same file* still
collide at the **text layer** (the editor buffer, `git`). Non-overlapping symbols
are **not** non-overlapping text. A primitive that looks like a mutex but cannot
prevent a text-level stomp grants **false safety** — which is worse than no lock,
because agents will trust it and lose work.

So the design question is not "how do we build symbol locks" but "what can an
intent plane honestly guarantee about a shared file, and how do we name it so no
one assumes more."

## Decision

Do **not** add symbol/resource leases. Ship and document **progressive task
handoffs** using the existing task CAS plus an explicit dependency:

- `create_task(..., blocked_by=<predecessor>)` creates a successor in `blocked`.
- A predecessor and successor must serve the same goal; each predecessor may
  have only one direct successor, and cycles/self-dependencies are rejected.
- The successor cannot be claimed and is excluded from `next_task`.
- Only an evidence-backed `complete_task` transition to `done` clears the
  dependency and opens direct successors, in the same SQLite transaction as the
  conformance audit.
- `in_review`, `blocked`, violation, lease expiry, and manual release do not open
  successors.

This serializes same-file ownership at the task layer. It does not claim that
symbols are independent text regions, and it adds no primitive that users might
mistake for a filesystem mutex.

The deterministic two-connection benchmark records the controlled contrast:

| Arm | Concurrent owners | Early successor claim | Maximum same-file owners | Collision risk |
|---|---:|---:|---:|---|
| Independent tasks | 2 | n/a | 2 | Present |
| `blocked_by` handoff | 1 | Rejected | 1 | Absent |

Result: [2026-07-23-progressive-handoff.json](../../benchmarks/results/2026-07-23-progressive-handoff.json).
This uses synthetic but schema-valid conformance evidence and proves the
coordination mechanism, not that autonomous agents always choose the pattern.
Add advisory leases only if a later real-agent scenario shows agents
still selecting colliding same-file work despite dependency instructions.

### Alternative considered and rejected (for now)

A **real** lock: Lodestar owns the writes, accepts AST-scoped patches, and does
server-side 3-way merge of concurrent same-file edits. This would make the
guarantee real, but it turns a small stdio intent plane into a write-coordinator
and mini-VCS — a large new surface, new failure modes, and coupling to file
contents the plane deliberately avoids (ADR-0004 keeps a loose node-id seam).
Out of scope; revisit only if advisory partitioning proves insufficient in
practice.

## Consequences

- **Positive.** The safe pattern is enforceable and transactional, reuses proven
  task claims/conformance, and cannot be confused with a text lock.
- **Cost.** Planning same-file work requires an explicit dependency chain; truly
  independent files may still use parallel open tasks.
- **Risk.** Agents may ignore the pattern and create independent same-file tasks.
  The board makes that visible, but no filesystem mutex exists. A real-agent
  adherence scenario remains required before closing that behavioral risk.
- **Invariants preserved.** Lodestar coordinates intent rather than owning file
  writes or merging text; the loose MindLeak node-id seam remains unchanged.

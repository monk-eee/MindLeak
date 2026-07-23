# ADR-0015: Advisory symbol-scoped leases for intra-file parallelism

- Status: Proposed (deliberately scoped — read the non-guarantee before building)
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

Model symbol leases as **advisory intent partitioning, explicitly not a mutex.**

- **Resource leases.** Generalize the existing task-lease machinery to a
  `resource_leases` table keyed by a node id (`symbol:` or `artifact:`), with
  `owner`, `lease_expires_at`, a caller-supplied short TTL, and `renew` — reusing
  the same atomic CAS and expiry that tasks already use.
- **Overlap + ancestry rule.** A claim is rejected if the resource is currently
  leased by another agent, **or** if its id is an ancestor/descendant of an
  active lease: a `symbol:src/lib.rs:Foo` lease conflicts with an
  `artifact:src/lib.rs` lease (the file contains the symbol), and vice versa.
  Two non-overlapping symbol leases in the same file coexist.
- **Explicit non-guarantee (must appear in the tool description verbatim in
  spirit).** A symbol lease answers *"is another agent currently intending to
  touch this symbol?"* so agents self-partition. It does **not** serialize file
  writes and does **not** prevent a text-level conflict. It is advisory.
- **The safe pairing is progressive handoffs.** For same-file multi-pass edits,
  the recommended pattern is `decompose_goal` into atomic per-symbol subtasks
  where `complete_task` releases before the next agent claims — which makes the
  file edits **serialize by task** rather than truly concurrent multi-writer.
  Advisory symbol leases *assist* this; they do not replace it.

### Alternative considered and rejected (for now)

A **real** lock: Lodestar owns the writes, accepts AST-scoped patches, and does
server-side 3-way merge of concurrent same-file edits. This would make the
guarantee real, but it turns a small stdio intent plane into a write-coordinator
and mini-VCS — a large new surface, new failure modes, and coupling to file
contents the plane deliberately avoids (ADR-0004 keeps a loose node-id seam).
Out of scope; revisit only if advisory partitioning proves insufficient in
practice.

## Consequences

- **Positive.** Agents can partition intra-file work and avoid both grabbing the
  same symbol; reuses the proven lease/TTL/renew CAS; is honest about its limits.
- **Cost.** A `resource_leases` table plus the ancestry/overlap check, and a new
  claim/renew/release surface at resource granularity. More tools on an already
  broad Lodestar surface.
- **Risk (the important one).** If shipped or marketed as a "lock," users assume
  conflict-free concurrent same-file writes and get burned. Mitigated **only** by
  naming (advisory *lease*, never *lock*) and stating the non-guarantee in the
  tool text and docs. If we cannot commit to that naming discipline, do not ship
  this.
- **Invariants preserved.** The plane stays a coordinator of *intent*, not an
  owner of file contents; the node-id seam (ADR-0004) stays loose.

## Open question (decide before building)

Do we ship the primitive at all, or just **document the progressive-handoff
pattern** — which already delivers safe same-file serialization with **zero new
primitives** using today's `decompose_goal` / `claim_task` / `complete_task` /
`next_task`? Advisory symbol leases add convenience (self-partitioning hints) but
also add surface and a false-safety risk. Recommendation: **document the pattern
first; build resource leases only if measured multi-agent runs show agents
genuinely colliding on symbol selection** despite the pattern.

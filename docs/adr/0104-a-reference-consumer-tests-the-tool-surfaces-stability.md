# ADR-0104: A reference consumer tests the tool surface's stability

- Status: Accepted
- Date: 2026-08-18
- Deciders: MindLeak maintainers
- Accepted: 2026-08-19 by the repository owner, authorized directly in
  session — attributed human adoption after review.
- Related: [ADR-0059](0059-the-tool-surface-is-a-vocabulary.md) (the tool
  surface is a vocabulary), [ADR-0103](0103-the-mcp-client-is-packaged-once-not-reimplemented.md)
  (the MCP client is packaged once, not reimplemented — this gate's natural
  companion), [ADR-0049](0049-publication-requires-a-claim.md) (precedent:
  a voluntary check is not a check), [ADR-0091](0091-ackplane-builds-and-tests-without-a-database.md)
  (existing precedent for a CI-side compatibility/build gate)

## Context

Nothing in this repository's CI currently proves that a change to
`mindleak-mcp`/`lodestar-mcp`'s tool surface is backward compatible for an
external consumer. ADR-0059 establishes that the tool surface is a contract
and versions `server_version` accordingly, but a contract checked only by a
human reading a diff is exactly the "decoration that reads as governance"
pattern ADR-0049 already measured and rejected for the Intent Plane's own
claim mechanism — the same failure mode, here applied to the MCP surface
instead of the task ledger.

This is concretely provable, not hypothetical: CompLeak's
`scripts/check-mindleak-connection.mjs` is a real, minimal external consumer
that connects to both servers, opens a session, and lists tools — and it
currently lives outside this repository. A breaking change here would surface
first as a CompLeak failure, discovered after the fact, rather than as a
MindLeak CI failure caught before merge.

This is the natural CI-side companion to ADR-0103: publishing a client raises
the stakes of breaking the protocol, because there is now a real, packaged
dependent to break.

## Decision

**CI runs a reference consumer against every change to either server's tool
surface, using only the published contract — never the servers' internal Rust
APIs.**

1. **The reference consumer is a separate, minimal client** — initially
   ADR-0103's packaged client if it exists by implementation time, or a
   standalone script otherwise — that spawns both servers exactly as an
   external process would, over stdio, and exercises `initialize`,
   `open_session`, `tools/list`, and one representative read-only call per
   tool family (a graph read, a task read, a knowledge read, a conformance
   read).
2. **The gate runs in the existing CI matrix, alongside `cargo test`, not as a
   separate manual step** — matching this project's own "do your laundry
   locally, CI is the safety net" discipline. A compatibility break should be
   caught the same automatic way a clippy warning is, not by a human
   remembering to check.
3. **A failing reference-consumer run blocks merge the same way a failing
   test does.** It is not advisory: an external consumer either can still
   complete session bootstrap and read every tool family, or the change is a
   breaking one that needs a version bump and, per ADR-0059, an explicit
   reviewed decision — not a silent merge.
4. **The gate tests the wire contract, never internal Rust types.** It talks
   to the compiled server binaries over stdio exactly as CompLeak or any
   other consumer would; it must not import `mindleak-core`/`lodestar-core`
   directly, or it stops proving what it claims to prove.
5. **New tools are additive by default and do not fail the gate.** A tool
   disappearing, a required argument becoming stricter, or a previously
   successful call now erroring are the failures this gate exists to catch —
   consistent with ADR-0059's own "reductions are permitted; an increase
   requires explicit review" framing for the tool-surface-budget clause.

## Consequences

- One more CI job, running both server binaries a second time in a black-box
  mode — a small, bounded cost against catching an external-breaking change
  before merge rather than after a downstream consumer reports it.
- The reference consumer must be kept in sync with which tool families exist;
  a genuinely new tool family needs a deliberate addition to the gate, a
  reasonable, visible cost rather than a hidden one.
- Gives ADR-0103's packaged client something tested continuously against,
  rather than a one-time hand check at release time.

## Rejected alternatives

- Relying on `docs/TOOLS.md` review alone — rejected: a human reading a diff
  is exactly the voluntary-consultation failure mode ADR-0049 already measured
  and rejected for claims; the same argument applies to protocol changes.
- Testing only through the existing Rust integration tests — rejected: those
  call the facade/store directly, proving the *implementation* works, not
  that the *wire contract* an external stdio consumer depends on still holds.
- Making this check advisory only — rejected: a contract that can be silently
  broken because the check is advisory is the same decoration-as-governance
  problem this ADR exists to avoid.

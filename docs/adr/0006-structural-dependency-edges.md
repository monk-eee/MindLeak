# ADR-0006 — Structural & dependency edges (graph enrichment for impact analysis)

- **Status:** Accepted
- **Date:** 2026-07-22
- **Implementation:** Phases 1-2 shipped 2026-07-22 for static
  JavaScript/TypeScript imports, package nodes, named cross-file calls, and
  simple named class/interface hierarchy.

## Context

The memory plane currently emits only *in-file* structural edges: `contains`
(artifact → symbol) and in-file `calls` (symbol → symbol, same file). That makes
`get_impact_radius` **shallow** — it cannot answer "if I change `auth.ts`, which
*other files* break?", because there are no cross-file edges.

It also starves [ADR-0005](0005-signal-weighted-decay.md). Two of its signal
proxies — **structural centrality** ("load-bearing, not a leaf") and
**corroboration** ("independent sources implicate the same node") — are
unmeasurable without a real dependency/type graph. Richer structure is not just a
feature; it is the substrate signal-weighted decay needs.

Seven richer relations were proposed (`imports`, `depends_on`, `implements`,
`extends`, `references`, `consumes`, `produces`). They differ sharply in how
cheaply and *deterministically* they extract — and zero-token determinism
([SPEC.md §1](../SPEC.md) invariant 1) is non-negotiable.

## Decision

Add **deterministic structural/dependency edges**, extracted by the same
pattern-based `ingest::ast` plus a small `ingest::manifest`. Nothing here touches
an LLM. Ordered by value × determinism:

- **`imports`** (Artifact → Artifact | Package) — parse `use` / `import` /
  `require` / `using` / `from` statements. Relative specifiers resolve to
  `artifact:` ids; bare specifiers become a new **`package`** node. This is the
  cross-file backbone.
- **cross-file `calls`** — a call whose callee is not defined in-file but *is* in
  the file's import table resolves to the imported symbol/file. Upgrades the
  existing in-file resolver; **no new relation**.
- **`extends` / `implements`** (Symbol → Symbol) — inheritance / conformance:
  `impl T for S`, `class X extends Y`, `class X implements I`, `class X(Base)`,
  supertrait `trait X: Y`.
- **`depends_on`** (Artifact → Package) — project-level, parsed from manifests
  (`Cargo.toml`, `package.json`, `go.mod`, `requirements.txt`).

New node type **`package`** (`package:<name>`) for external, non-workspace deps.

### Rejected / deferred

- **`references`** — a generic "mentions" edge connects everything to everything;
  it bloats the graph and **dilutes decay ranking** (the property that beats
  vector search). Deferred; if ever added, tightly scoped to *signature
  type-references only*, never bare identifiers.
- **`consumes` / `produces`** — architecture-level (HTTP endpoints, DB/queue, IO)
  need domain-specific extractors and are lower-confidence. A separate future ADR
  and a dedicated `ingest::runtime` extractor, not a regex bolted onto `ast`.

### Determinism & resolution

Import target resolution is **heuristic** (extension / index-file guessing). We
emit a best-guess **stub** `artifact:` node plus its deterministic candidate ids.
Normal ingestion promotes exact matches or any candidate match, atomically
retargets owned `imports` and resolvable `calls`, then removes the orphan stub.
This remains heuristic; Tree-sitter is the precision upgrade (ADR-0002).

### Decay

Structural/dependency edges are durable → the **168h** tier. They are not
immortal. Current file structure is authoritative: ADR-0007 retracts a deleted
import immediately on the next ingest. Decay still governs unrefreshed structural
evidence between snapshots, while ADR-0005 may later extend proven load-bearing
relations.

## Consequences

- `get_impact_radius` becomes genuinely **cross-file**: change a symbol → its
  file → files that `import` it → their symbols. This is the headline payoff.
- ADR-0005 gains its substrate: `imports` / `depends_on` / `extends` /
  `implements` make **structural centrality** and **corroboration** computable.
- **Do not** add `references` / `consumes` / `produces` as cheap regexes on the
  hot path — the determinism/noise cost is real; they are separate, gated work.
- **Do not** hard-require exact import resolution; **stub-and-reconcile** is the
  contract. A dangling best-guess is acceptable, self-heals when a candidate is
  ingested, and is deleted when its final import disappears.
- Import/manifest parsing stays zero-token (invariant 1), off any LLM path.
- New tests: cross-file `imports` + `calls` resolution, `package` node creation,
  `extends`/`implements` extraction, manifest `depends_on`, reconcile-by-id.

## Phasing

1. `imports` + `package` + cross-file `calls` (shipped).
2. `extends` + `implements` (shipped for JS/TS simple named heritage).
3. `depends_on` from manifests (planned).

Phases 1-2 currently support static JS/TS `import` and `require` syntax plus
local and named-import class/interface heritage. Generic constraints are not
misclassified as inheritance; expression-based mixins and default/namespace
heritage resolution are explicitly unsupported. Other language syntaxes,
default/namespace call resolution, path aliases, and re-exports remain future
fixtures rather than implied support.

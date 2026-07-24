# AGENTS.md — MindLeak Codebase Guide for AI Agents

MindLeak is a **Temporal Context Graph Engine (TCGE)**: a local, decay-weighted
knowledge graph that gives coding agents durable context, replacing flat-log /
vector-only agent memory. Read [`docs/SPEC.md`](docs/SPEC.md) for the design and
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for the module map before making
structural changes.

---

### Core coding philosophy

> "The code you write makes you a programmer.
> The code you delete makes you a good one.
> The code you don't have to write makes you a great one."
> — Mario Fusco

Lines of code are a cost, not an asset. The best contribution is often a
smaller diff than you arrived expecting to write — a delete, a consolidation,
or a one-line addition to an existing function instead of a new sibling.

#### Before-you-write checklist (NON-NEGOTIABLE)

Run these checks before writing any helper, method, or "small" function:

1. **Grep the crate for the behaviour.** `grep -rn "fn <verb>" crates/` — if
   something already does this, call it. If a near-miss exists, extend it — do
   not fork a parallel helper.
2. **Check the facade and the shared modules.** The `MindLeak` facade in
   [`lib.rs`](crates/mindleak-core/src/lib.rs) is the public surface; graph reads
   /writes belong on `GraphStore` ([`graph.rs`](crates/mindleak-core/src/graph.rs));
   deterministic ingestion helpers (`short_hash`, `normalize_path`, `clamp`) live
   in [`ingest/mod.rs`](crates/mindleak-core/src/ingest/mod.rs). Add there and
   call it — don't paste the same three lines into a fourth module.
3. **Check the red-flag shapes.** A new free function taking the same first two
   or three arguments at every call site is a method in disguise. A `static mut`
   is a class without the class. A second `*_or_default` / `*_safe` / `*_retry` /
   `*_v2` beside an existing one is a fork waiting to happen. Look again, harder.
4. **Write it twice → extract immediately.** If you find yourself writing the
   same helper a second time in one session, that second occurrence is the signal
   to extract it now. "Once more and clean up later" — later does not arrive.

---

### Prime directive (READ FIRST, OVERRIDES EVERYTHING BELOW)

**Do the right thing, not the expedient thing.** When a clean design and a quick
hack both reach green tests, pick the clean design. When fixing one test the
right way would require updating fifteen others, update the fifteen — do not add
a back-compat shim, a transitional bridge, a "for now" indirection, or a fallback
that quietly preserves the legacy pattern. Those shortcuts calcify: they ship
with TODO comments that never get resolved, and the next agent inherits two ways
to do the same thing forever.

Concrete tells you are about to take the expedient path:
- "I'll add a fallback so legacy callers keep working" — no, migrate the callers.
- "Tests assert against the old constant; I'll make the new code read both" — no,
  update the tests.
- "This is a bridge until the wider refactor lands" — the bridge becomes
  permanent. Land the refactor now or do not introduce the new abstraction yet.
- "Touching 15 files for one design change is too much" — if the design is right,
  that is what it costs. Pay it.
- A `// TODO: remove once X migrates` comment in a commit that does not also do X.

If the right thing is genuinely too large for one commit, **stop and say so** —
do not ship the expedient half. Reduce scope to a smaller right-shaped change, or
split into a sequence of right-shaped commits, each individually principled. The
wrong unit of work is "the right design plus a hack to make CI green".

---

## Domain invariants (the load-bearing rules)

These are MindLeak's hard constraints. Breaking one is a design incident, not a
style nit.

1. **Zero-token write path.** All ingestion (`execution`, `git`, `ast`) stays
   deterministic — pattern matching only, no LLM tokens. LLM calls live
   *exclusively* in the async consolidation layer
   ([`consolidate.rs`](crates/mindleak-core/src/consolidate.rs)). **Never** add an
   LLM call to the ingest or query hot path.
2. **Decay is the point.** Do not "fix" stale context by disabling decay. Edges
   are meant to fade; tune half-lives (`RelationType::default_half_life_hours`)
   rather than removing the mechanism.
3. **Effective weight is derived, never stored.** Compute it at query time via
   the `effective_weight()` SQL function or `decay::effective_weight`. Do not add
   a background job that rewrites edge weights row-by-row.
4. **The consolidation LLM is OpenAI-compatible and optional.** It speaks
   `/v1/chat/completions` (Ollama's `/v1`, LM Studio, llama.cpp, …) and must error
   cleanly when no server is reachable. Never make the deterministic path depend
   on it.

---

## Vibe coding rules (mandatory for all AI agents)

### File discipline
- **Small, focused modules.** Prefer splitting a module over growing a file past
  a few hundred lines. snake_case Rust modules; one clear responsibility each.
- **Keep the surface tight.** Only make items `pub` that are actually called from
  outside the module; a `pub fn` nobody calls is dead surface — delete it.

### Ask before acting (NON-NEGOTIABLE, ADR-0029)
- **Consult the constitution before you touch code.** At claim time, and before
  editing any `governed`/`forbid_change` file, call Lodestar's `advise` (or read
  the governing clauses surfaced on `claim_task` / `next_task`) with the
  `artifact:`/`symbol:` ids you intend to change. It returns the clauses that
  govern that scope and a proportional disposition — `advise` (proceed, honour
  the clauses), `review` (you would drift outside a covering task — get one
  first), `block` (a `forbid_change` lock — needs a waiver, not a commit), or
  `needs_human` (no constitution adopted).
- **`advise` informs; it never gates.** It is evidence-free, records no verdict,
  changes no task state, needs no model, and never blocks a compare-and-swap
  claim. Skipping it does not dodge the verdict — retrospective conformance at
  `complete_task` (ADR-0009/0025) is still the backstop that lands drift or a
  violation in review or blocked. The point is to catch it *before* you do the
  work, not after.

### Test-driven workflow (NON-NEGOTIABLE)
- **Tests are the only way we ship.** Every new tool, parser, or facade method
  gets a test — colocated `#[cfg(test)] mod tests` in the module, or an
  integration test in
  [`tests/integration.rs`](crates/mindleak-core/tests/integration.rs).
- **Run tests after every change.** Even minor edits introduce side effects.
- **Never skip, disable, or delete a test to make CI pass.** Fix the code.
- **Bug fixes require a regression test. No exceptions.** It must FAIL against the
  un-fixed code and PASS against the fix — confirm both directions before
  committing. The test name + a comment describe the bug in plain English: what
  went wrong, the impact, and the fix.
- **Extension behaviour gets a vitest test.** Pure logic in
  [`editors/vscode/src/util.ts`](editors/vscode/src/util.ts) is unit-tested; keep
  vscode-coupled code thin so it stays testable.
- **Always report bugs and failures, even ones you do not fix this run.** If you
  spot a bug, a flaky test, or a latent footgun while doing other work, add it to
  the **Known gaps** section of [`DEVELOPERS.md`](DEVELOPERS.md) before you finish:
  (1) what you observed, (2) where (file + symbol or test name), (3) impact,
  (4) fixed this run or left for later. **We never silently drop bugs.**

### Pre-commit hooks (NON-NEGOTIABLE)
- **Hooks run on every commit** — rustfmt, clippy (`-D warnings`), eslint,
  prettier, whitespace, and JSON/TOML validity; the test suite runs on push.
- **Install once:** `make setup` (or `pre-commit install && pre-commit install
  --hook-type pre-push`). If you skip it the hooks are silently bypassed and debt
  accumulates.
- **Do your laundry locally.** CI is the safety net, not the first line of
  defence. **Never use `--no-verify`** — a skipped hook is deferred cost with
  interest.

### Git discipline
- **One commit = one meaningful unit of work.** Scoped, validated, tested.
- **Conventional Commits** (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`,
  `chore:`). The commit body is a good place for `DECISION:` / `WHY:` rationale
  markers — MindLeak ingests those into intent nodes.
- **Stage explicitly with named paths** and review every diff before committing —
  do not blindly accept generated code. Never `git add -A` a mixed working tree.
- **One checkout, one fleet branch, one publisher (ADR-0032).** Agents share the
  primary checkout and one `fleet/<goal>` branch. Do not create Git worktrees and
  do not cherry-pick routine work. Only the designated integrator may fetch,
  reconcile, push with `node scripts/canonical-push.mjs`, or update the pull
  request; every other agent edits and makes scoped commits only.
- **Divergence stops the fleet.** If the remote branch is not an ancestor of
  `HEAD`, stop taking work, finish or release current claims, reach a scoped clean
  checkpoint, and reconcile once in the primary checkout. Never move `main`
  underneath dirty files and never publish from a side lineage.

### Doc discipline (NON-NEGOTIABLE)
Doc drift is treated like a failing test. A shipped change updates the relevant
surface in the same commit:
- [`CHANGELOG.md`](CHANGELOG.md) — any user- or operator-visible change (Keep a
  Changelog format, under `## [Unreleased]`).
- [`docs/SPEC.md`](docs/SPEC.md) — if it changes the design contract.
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — if it adds a module/capability.
- [`README.md`](README.md) tool table — if it adds/removes an MCP tool.
- [`docs/adr/`](docs/adr/) — a decision that is hard to reverse or surprising gets
  an ADR; do not bury architecture decisions in a code comment.

A purely internal refactor (file split, helper extraction) only needs a CHANGELOG
line if it is observable; otherwise no doc change is required.

### Code reuse (NON-NEGOTIABLE)
- **Check the existing modules before writing a new helper.** Graph access flows
  through `GraphStore`; ingestion utilities live in `ingest/mod.rs`; decay math
  lives in `decay.rs`. If a capability is missing, **add it to the right module
  first**, then call it.
- **If a near-miss exists, extend it** rather than forking. When you spot a
  duplicate during unrelated work, file it in the Known gaps of `DEVELOPERS.md`
  instead of silently leaving it for the next agent.

### Code design discipline (NON-NEGOTIABLE)
Idiomatic, testable Rust by default.
- **Dependency injection over globals.** State with identity (the SQLite
  connection, the graph) lives on `GraphStore`, constructed once and passed by
  reference (`&GraphStore`) — not reached for through a `static`. Tests build
  their own with `MindLeak::open_in_memory()`; no global to monkey with.
- **Derived, not stored.** Effective edge weight is computed, never persisted
  (see invariant 3). Any value that is a pure function of other state is computed
  at read time.
- **Errors are typed.** `MindLeakError` in the core library; `anyhow` in the
  binaries. No `unwrap()` / `expect()` on fallible I/O outside tests.
- **Exhaustive `match` on enums.** Prefer matching every `NodeType` /
  `RelationType` variant over a catch-all `_` that silently swallows a new one.
- **Structs / enums for value objects**, `#[derive(...)]` liberally. A struct
  wrapping one pure function is a smell — write the free function instead.
- **Anti-patterns to refuse:** storing a derived weight; an LLM call on the
  ingest/query path; a "temporary" back-compat shim; a `_v2` function beside the
  original; a test that pokes private state because there is no injectable seam
  (fix the seam, not the test).

### Safety and secrets
- **Never log tokens, PATs, or credentials.** The optional LLM API key
  (`MINDLEAK_LLM_API_KEY`) is read from the environment and never logged.
- **`mindleak-mcp` is stdio-only and unauthenticated by design** — it has no
  network listener. Do not add one without an auth layer (see
  [`docs/SPEC.md` § 8](docs/SPEC.md)). Treat `.mindleak/graph.db` as workspace-
  sensitive (it is gitignored and regenerable).

### Toolchain (NON-NEGOTIABLE — platform-agnostic only)
- **Everything must run identically on Linux, macOS, and Windows.** Use `cargo`,
  `npm`, `make`, and `git` — they are cross-platform. **Do not** put
  PowerShell-only, `cmd`-only, or bash-only invocations into scripts, Makefile
  targets, CI, or docs. Cross-platform build steps go through a portable runner
  (e.g. the Node script in [`editors/vscode/scripts/`](editors/vscode/scripts/)),
  never a shell one-liner that only works on one OS.

---

## Tech stack

| Layer | Choice |
|---|---|
| Core + server | Rust 2021 (edition), `rusqlite` (bundled + FTS5 + functions) |
| Ingestion | `regex`, `sha2` — deterministic, no LLM tokens |
| LLM (optional) | any OpenAI-compatible server (`/v1`) via `ureq`, async only |
| Server transport | hand-rolled newline-delimited JSON-RPC 2.0 (MCP) |
| Extension | TypeScript, VS Code Webview API, vendored Cytoscape.js |
| Storage | single-file SQLite (`.mindleak/graph.db`, WAL), regenerable |

## Project structure

```
MindLeak/
├── AGENTS.md                       # this file — agent grounding
├── README.md                       # front door / router
├── DEVELOPERS.md                   # clean-machine-to-running + Known gaps
├── docs/                           # SPEC · ARCHITECTURE · CONTRIBUTING · adr/
├── crates/
│   ├── mindleak-core/              # the engine (Rust lib)
│   │   └── src/
│   │       ├── lib.rs              # `MindLeak` facade — public surface
│   │       ├── model.rs            # Node / Edge / NodeType / RelationType
│   │       ├── schema.sql          # tables + FTS5 + triggers
│   │       ├── db.rs               # connection, migrations, effective_weight() fn
│   │       ├── decay.rs            # half-life decay + prune threshold
│   │       ├── graph.rs            # GraphStore: upsert, traverse, snapshot, prune
│   │       ├── ingest/             # zero-token extractors: execution · git · ast
│   │       └── consolidate.rs      # optional OpenAI-compatible consolidation
│   ├── mindleak-mcp/               # MCP stdio server (Rust bin)
│   │   └── src/                    # main · server · tools
│   ├── lodestar-core/              # Intent Plane — durable spec brain (ADR-0004)
│   │   └── src/                    # lib · model · store (claim/lease) · llm
│   └── lodestar-mcp/               # Intent Plane MCP server (Rust bin)
│       └── src/                    # main · server · tools
└── editors/vscode/                 # passive sensor + Cytoscape visualizer (TS)
```

## Adding an MCP tool (worked path)

1. Method on the `MindLeak` facade — [`crates/mindleak-core/src/lib.rs`](crates/mindleak-core/src/lib.rs).
2. Definition in `list()` + branch in `call()` —
   [`crates/mindleak-mcp/src/tools.rs`](crates/mindleak-mcp/src/tools.rs).
3. Integration test.
4. README tool-table row + CHANGELOG line.

## Commands (identical on every OS)

Prefer the Unit Test MCP tools for test runs where available; otherwise these
work the same on Linux, macOS, and Windows:

```bash
make setup          # install pre-commit hooks + extension deps
make build          # cargo build
make test           # cargo test --all
make fmt            # cargo fmt --all
make clippy         # cargo clippy --all-targets --all-features -- -D warnings
make lint           # fmt-check + clippy + extension eslint
make ci             # everything CI runs

# Direct equivalents (no shell-specific syntax):
cargo build
cargo test --all
cargo clippy --all-targets --all-features -- -D warnings
npm --prefix editors/vscode run compile
npm --prefix editors/vscode run lint
npm --prefix editors/vscode test
```

Full command table: [`DEVELOPERS.md`](DEVELOPERS.md).

## Conventions

- **File naming:** snake_case for Rust modules, kebab-case for crate names,
  camelCase for TypeScript files.
- **Node ids:** stable and human-readable —
  `artifact:<path>`, `symbol:<path>:<name>`, `execution:<hash>`, `intent:<sha|hash>`.
- **Paths:** always normalise to forward slashes (`ingest::normalize_path`) before
  building ids.
- **No emojis** in Rust / graph content. (The extension UI legend is the only
  place emojis appear, and that's UI copy.)

## Gotchas

- `rusqlite` needs features `["bundled", "functions"]` — `functions` is required
  for the `effective_weight` scalar registration.
- MCP stdio is **newline-delimited** JSON-RPC (not Content-Length framed). Drive
  the server by piping one JSON object per line to its stdin from any harness.
- FTS5 search input is sanitised in `graph::build_fts_query` — keep queries going
  through it to avoid MATCH syntax errors.

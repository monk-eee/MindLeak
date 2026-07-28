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

1. **Check who else is already in there.** `check_overlap(paths, session_id)` —
   it names the agents whose recent, decay-weighted footprints touch the files
   you are about to edit. Use it before editing any shared file (`AGENTS.md`,
   `DEVELOPERS.md`, `CHANGELOG.md`, `docs/ARCHITECTURE.md`, anything in
   `scripts/`), and before claiming work that spans them. Measured: the dominant
   collision in this fleet is two agents editing neighbouring lines of the same
   file, and `check_overlap` names them correctly *before* the merge conflict
   rather than after.
   This is the only graph read the checklist mandates. `recall` and
   `get_impact_radius` are deliberately **not** here — see Known gaps in
   [`DEVELOPERS.md`](DEVELOPERS.md); neither can currently answer a
   before-you-write question about Rust, and mandating a tool that returns
   plausible strangers would only teach you to ignore this list.
2. **Grep the crate for the behaviour.** `grep -rn "fn <verb>" crates/` — if
   something already does this, call it. If a near-miss exists, extend it — do
   not fork a parallel helper.
3. **Check the facade and the shared modules.** The `MindLeak` facade in
   [`lib.rs`](crates/mindleak-core/src/lib.rs) is the public surface; graph reads
   /writes belong on `GraphStore` ([`graph.rs`](crates/mindleak-core/src/graph.rs));
   deterministic ingestion helpers (`short_hash`, `normalize_path`, `clamp`) live
   in [`ingest/mod.rs`](crates/mindleak-core/src/ingest/mod.rs). Add there and
   call it — don't paste the same three lines into a fourth module.
4. **Check the red-flag shapes.** A new free function taking the same first two
   or three arguments at every call site is a method in disguise. A `static mut`
   is a class without the class. A second `*_or_default` / `*_safe` / `*_retry` /
   `*_v2` beside an existing one is a fork waiting to happen. Look again, harder.
5. **Write it twice → extract immediately.** If you find yourself writing the
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
  `node scripts/scoped-commit.mjs -m "<msg>" -- <paths>` enforces both.
- **Never commit from a working tree another agent is writing to.** `pre-commit`
  stashes every unstaged change before running hooks and restores it afterwards.
  That is safe alone and corrupting in a fleet: if the other agent writes inside
  that window, the restore collides and the hooks report a *fictitious* failure —
  `files were modified by this hook`, on a hook that modifies nothing, naming a
  file you never touched. The message points nowhere near the cause, which is
  what makes it so expensive to unpick. `scoped-commit` refuses this (exit 3)
  when more than one worktree is attached and unstaged files outside your
  declared paths are live. If you are the only operator, `--allow-foreign-wip`.
- **One isolated worktree and branch per concurrent workstream (ADR-0038).** Git
  isolates files, the index, and branch selection; Lodestar coordinates claims
  and proof; MindLeak shares repository learning. Sharing one writable checkout
  is a reviewed exception, not the default. Do not cherry-pick, rebase, or squash
  routine work: those operations replace evidence-bearing commit identities.
- **Publish exact commits; converge through review.** Any clean attached worktree
  may publish its current non-protected branch with
  `node scripts/canonical-push.mjs`. The script pushes exact `HEAD` to the same
  branch name and refuses dirty state or remote divergence. Only a protected,
  policy-compliant pull-request merge may advance `main`.
- **Divergence stops that branch.** If the remote branch is not an ancestor of
  `HEAD`, stop work on that branch, reach a clean checkpoint, and reconcile in
  its own worktree. Never move refs underneath dirty files or repair routine
  divergence by manufacturing replacement commits.
- **Arm it and leave it alone — the queue takes the turns (ADR-0062).** `main`
  requires branches to be up to date, so with several armed pull requests every
  merge makes all the others stale. If each agent refreshes its own branch the
  moment that happens, they collide continuously: N branches each burn a full
  check run against a `main` that the next merge invalidates again, and nothing
  drains. Eleven armed, green pull requests once sat unmerged for two hours that
  way.
  So **do not run `gh pr update-branch`, and do not merge `main` into a branch
  just because it went behind.** Enabling auto-merge is how you join the queue —
  armed means finished (ADR-0045) — and `scripts/delivery-queue.mjs` brings
  exactly one branch up to date at a time, in the order branches were armed. It
  never merges; merging stays with GitHub behind the same required checks.
  Merge `main` in yourself **only** when the queue reports a real conflict on
  your branch, which it cannot resolve for you — that is the one case it hands
  back, and it needs your worktree (see the divergence rule above).

### Doc discipline (NON-NEGOTIABLE)
Doc drift is treated like a failing test. A shipped change updates the relevant
surface in the same commit:
- [`changelog.d/`](changelog.d/) — any user- or operator-visible change adds
  `changelog.d/<section>-<slug>.md`. **Do not edit `CHANGELOG.md` in a pull
  request**: it is assembled at release, because a shared append-only file made
  auto-merge go stale on every concurrent branch (ADR-0056).
- [`docs/SPEC.md`](docs/SPEC.md) — if it changes the design contract.
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — if it adds a module/capability.
- [`docs/TOOLS.md`](docs/TOOLS.md) tool table — if it adds/removes an MCP tool.
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
  [`docs/SPEC.md` § 8](docs/SPEC.md)). Treat the per-repository user-local
  `graph.db` and `spec.db` as workspace-sensitive; inspect their resolved paths
  with `storage_status` (ADR-0038).

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
| Storage | per-clone repository id → user-local `graph.db` + `spec.db` (SQLite WAL) |

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
2. Definition in `definitions()` + branch in `call()` — the matching module under
   [`crates/mindleak-mcp/src/tools/`](crates/mindleak-mcp/src/tools/).
3. Integration test.
4. [`docs/TOOLS.md`](docs/TOOLS.md) tool-table row + a `changelog.d/` fragment.

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

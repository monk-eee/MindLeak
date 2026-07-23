# Developing MindLeak

From a clean machine to the engine building, tested, and the extension running.
If you get stuck, that is a defect — fix it or add it to [Known gaps](#known-gaps).

## Prerequisites

- **Rust** 1.75+ (via [rustup](https://rustup.rs)); MSVC toolchain on Windows.
- **cargo-llvm-cov** for local Rust coverage (`cargo install cargo-llvm-cov --locked`).
- **Node** 18+ and npm (for the VS Code extension).
- **Python** 3.8+ with `pip` (for the `pre-commit` framework).

## One-time setup

```bash
git clone https://github.com/monk-eee/MindLeak
cd MindLeak

# Rust components
rustup component add rustfmt clippy
cargo install cargo-llvm-cov --locked

# Pre-commit hooks (client-side enforcement)
pip install pre-commit
pre-commit install
pre-commit install --hook-type pre-push

# Extension dependencies
npm --prefix editors/vscode install
```

On systems with `make`, `make setup` does the hook + extension steps.

The cargo hooks are **scoped and isolation-aware** (`scripts/cargo-precommit.mjs`):
they run `cargo fmt/clippy/test` only for the crate packages your change touches,
and — on push, or when a foreign untracked file sits in an affected crate —
validate a throwaway worktree snapshot instead of the shared working tree. This
keeps a concurrent agent's broken crate or uncommitted WIP from failing your
commit or push in a shared checkout (ADR-0018).

Two helpers make the safe path the easy path when agents share one checkout:
`node scripts/scoped-commit.mjs -m "<msg>" -- <path>...` stages and commits only
your declared paths (never `git add -A`), and `node scripts/isolated-push.mjs`
pushes the current commit through the hooks from a throwaway worktree so another
agent's broken WIP cannot poison your push.

**Success looks like:** `cargo test --all` reports `test result: ok` for every
crate, and `target/debug/mindleak-mcp` starts and prints
`[mindleak-mcp] ready — graph at …` on stderr.

## Everyday commands

| Task | `make` | Direct command |
|---|---|---|
| Build | `make build` | `cargo build` |
| Test | `make test` | `cargo test --all` |
| Coverage | `make coverage` | Rust LCOV + scoped Vitest coverage; both enforce an 80% floor |
| Format | `make fmt` | `cargo fmt --all` |
| Format check | `make fmt-check` | `cargo fmt --all -- --check` |
| Lint (Rust) | `make clippy` | `cargo clippy --all-targets --all-features -- -D warnings` |
| Lint (extension) | `make ext-lint` | `npm --prefix editors/vscode run lint` |
| Test (extension) | `make ext-test` | `npm --prefix editors/vscode test` |
| Compile extension | `make ext-compile` | `npm --prefix editors/vscode run compile` |
| Everything CI runs | `make ci` | see [`.github/workflows/ci.yml`](.github/workflows/ci.yml) |

> **`make` is optional.** Every target maps to the direct command in the
> right-hand column — `cargo`, `npm`, and `git` are identical on Linux, macOS,
> and Windows, so run those directly if `make` is unavailable.

## Local gate before a PR

Do your laundry locally — CI is the safety net, not the first line of defence:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
npm --prefix editors/vscode run lint
npm --prefix editors/vscode test
npm --prefix editors/vscode run compile
make coverage
```

## Publishing a binary release

The tag-driven [release workflow](.github/workflows/release.yml) publishes both
MCP servers for Windows x64, Linux x64, macOS Intel, and macOS Apple Silicon.
Each target gets a one-command installer archive and a VSIX containing both
native servers. The workflow reruns `make ci`, performs native MCP
initialization/tool-list smoke checks, packages runtime-only VSIX files,
attests the ZIP/VSIX assets, and publishes `SHA256SUMS`. CI separately runs a
live pinned VS Code 1.93.1 Extension Host smoke on Windows.

1. Update `[workspace.package].version` in [`Cargo.toml`](Cargo.toml), the VS Code
  package version, and the corresponding changelog/release notes.
2. Merge the release commit to `main` and confirm CI is green.
3. Create and push a matching tag:

```bash
git tag v0.1.0
git push origin v0.1.0
```

Prerelease tags such as `v0.1.0-preview.1` may share the base workspace version
`0.1.0`. A mismatched or malformed tag fails before any binaries are built.

## Pre-commit

Hooks run automatically on `git commit` (formatting, lint, whitespace, JSON/TOML
validity) and on `git push` (the test suite). Never bypass with `--no-verify`;
fix the code instead. Configuration: [`.pre-commit-config.yaml`](.pre-commit-config.yaml).

## Running the MCP server by hand

```bash
MINDLEAK_DB="$PWD/.mindleak/graph.db" cargo run -p mindleak-mcp
```

Then paste newline-delimited JSON-RPC requests on stdin, e.g.:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
```

> Pipe a request file to the server's stdin from any shell/harness — it reads
> one JSON object per line: `mindleak-mcp < in.jsonl > out.jsonl`.

## Debugging the extension

```bash
cargo build              # produce target/debug/mindleak-mcp(.exe)
npm --prefix editors/vscode run watch
```

Press **F5** in VS Code to launch an Extension Development Host. The extension
auto-detects the workspace `target/debug` or `target/release` binary.

## Environment variables

| Variable | Default | Used by |
|---|---|---|
| `MINDLEAK_WORKSPACE` | process working directory | project root for default graph/config paths |
| `MINDLEAK_DB` | `<workspace>/.mindleak/graph.db` | server graph location |
| `MINDLEAK_AGENT` | *(empty)* | agent id for attribution (`observed` edges); empty = off |
| `MINDLEAK_CONFIG` | `<workspace>/.mindleak.toml` | per-project decay policy |
| `MINDLEAK_WORKING_SET_SIZE` | `7` | hard cap for the current agent's derived working set (1-32) |
| `MINDLEAK_AUTONOMOUS_CONSOLIDATION` | `false` | explicit opt-in to idle model-backed consolidation |
| `MINDLEAK_CONSOLIDATE_IDLE_SECS` | `300` | idle trigger (30-86400) |
| `MINDLEAK_CONSOLIDATE_MIN_INTERVAL_SECS` | `3600` | minimum attempt interval (60-86400) |
| `MINDLEAK_CONSOLIDATE_MAX_NODES` | `20` | candidates per pass (1-200) |
| `MINDLEAK_LLM_URL` | `http://localhost:11434/v1` | consolidation server (OpenAI-compatible) |
| `MINDLEAK_MODEL` | `glm4:9b` | consolidation model |
| `MINDLEAK_LLM_API_KEY` | *(empty)* | bearer token for hosted LLM servers (optional) |

## Adding an MCP tool

1. Add a method to the `MindLeak` facade in [`lib.rs`](crates/mindleak-core/src/lib.rs).
2. Add a definition to `list()` and a branch to `call()` in
   [`tools.rs`](crates/mindleak-mcp/src/tools.rs).
3. Add a test in [`tests/integration.rs`](crates/mindleak-core/tests/integration.rs).
4. Add a row to the tool table in [`README.md`](README.md).

## Known gaps

Be honest — an empty Known Gaps section is almost always a lie. The rough edges
and footguns, with impact and status:

- **The Intent-Plane governance graph accumulates cross-goal code bindings.** —
  Over a long multi-agent session, repeated `link_goal_to_code` calls leave many
  lodestar source files bound to several goals at once (e.g.
  `crates/lodestar-mcp/src/tools/knowledge.rs` ends up governed by both
  `goal:durable-intent-plane-...` and `goal:local-temporal-context-graph`).
  `evaluate_conformance` flags any changed node bound to a *non-task* goal as
  Drift, so a correct commit serving one goal now reports drift against the
  others, and there is no `unlink_goal_from_code` verb to prune a stale binding. —
  Medium impact on conformance signal (false drift): the ADR-0022 wiring landed
  tested + pushed (commit `4267aaa`) but its conformance verdict is `drift`, not
  `aligned`, purely for this reason. — Left for later: add an unbind / govern-audit
  verb plus a one-time binding cleanup; observed on `task:85b9114ba31f`.

- **The real-agent product gate is narrow.** — Three runs per arm on one
  composite typed-session fixture with Copilot CLI 1.0.63 / Haiku 4.5 cross the
  exploration and success thresholds, but do not establish general performance
  across repositories, models, or long-running teams. The two-agent duplicate-
  work scenario is covered by the claim CAS proof, not this agent-loop result. —
  Medium impact on claim breadth. — Productization may proceed; broader external
  replications remain required for universal efficacy claims.

- **Signal consequence remains a bounded temporal proxy.** — A failure earns
  consequence only when the same command later succeeds after a related change,
  but this still cannot prove causality. The 8x cap, provenance-bearing handoff,
  and eventual decay limit coincidence laundering. — Medium impact on salience
  precision. — Left explicit; stronger causal tracing needs process/test
  attribution rather than another heuristic.
- **Derived signal queries are benchmarked, not asymptotically free.** — Evidence
  is computed per edge from graph state; a 200-edge snapshot measured 16.757 ms
  p95, but much larger dense graphs may need batched SQL/materialized raw
  provenance. — Low current impact. — Left as a measured scaling boundary.
- **Episodic edges previously used ingestion wall-clock time.** — Delayed passive
  execution/commit ingestion could invert failure/change/success chronology and
  fabricate or hide consequence. — High impact on signal correctness. — Fixed
  this run: execution and commit edges now use authoritative record timestamps,
  with regression tests.

- **Symbol and import extraction remains heuristic and partially scoped.** —
  Static JS/TS named imports now produce cross-file `calls`, but default and
  namespace calls, re-exports, path aliases, dynamic imports, and other language
  import syntaxes are not resolved. Type hierarchy supports simple named local
  and imported JS/TS heritage, not default/namespace targets or expression-based
  mixins. Non-JS brace/indent extractors also remain regex-based. — Medium impact
  on graph completeness. — Tracked: expand fixture-backed deterministic parsers;
  Tree-sitter remains the precision upgrade (ADR-0002).
- **Manifest dependency support is direct-only.** — `Cargo.toml`, `package.json`,
  `go.mod`, and named PEP 508 lines in `requirements*.txt` emit `depends_on`.
  Lockfiles, transitive dependencies, npm overrides, Cargo workspace catalogs,
  Go replacements, requirement includes/options, and unnamed VCS/local Python
  requirements do not. — Low impact on direct impact analysis; intentional to
  avoid turning catalogs and resolver output into false direct edges.
- **The live LLM round-trip runs only on demand, not in CI.** — Ignored tests
  (`cargo test -- --ignored`) exercise the real `/v1/chat/completions` call for
  both planes (MindLeak `consolidate`, Lodestar `decompose`/`judge`) against a
  running model; CI can't run them without one. — Low impact. — Running them
  surfaced (and fixed) that `glm4:9b` wraps its JSON in prose even with
  `response_format: json_object`; both clients now extract the JSON object
  robustly.
- **Ingest tools are unauthenticated (by design).** — Any client with stdio
  access to `mindleak-mcp` can write nodes/edges. — Acceptable for local
  single-user use; the server has no network listener. Do not expose it over a
  network without an auth layer (see [docs/SPEC.md § 8](docs/SPEC.md)).
- **Passive execution evidence depends on VS Code shell integration.** — VS Code
  1.93 shell start/end events provide command/exit evidence; unsupported or
  conflicting shells report degraded capture and are not guessed from terminal
  text. Concurrent terminal executions can both observe one workspace mutation,
  so changed paths prove temporal overlap rather than process-level causality. —
  Medium impact on provenance precision in overlapping command sessions.
- **Lodestar worktree sharing is path-based, not git-aware.** — The Intent Plane
  DB resolves from `LODESTAR_DB` else `<cwd>/.lodestar/spec.db`; sibling git
  worktrees share one plane only if pointed at the same path. — Low impact. —
  Git-common-dir auto-resolution is a documented follow-up (SPEC-INTENT §3).
- **Unit Test MCP 1.3.6 cannot validate this workspace reliably.** — Its Vitest
  discovery finds `src/util.test.ts`, but `run_tests` reports a passing total of
  zero even for that explicit path. On Windows, a backslash Cargo root is
  rejected as `INVALID_ROOT_DIR`; normalizing it to forward slashes runs the
  custom command and surfaces failures, but successful runs still report zero
  tests and no coverage. — High impact on local proof: MCP cannot establish test
  counts or coverage. — Left open in the external adapter; CI's test jobs and
  `cargo-llvm-cov`/Vitest coverage artifacts remain authoritative until repaired.
- **The extension toolchain has one low-severity development advisory.** —
  Vitest resolves `esbuild` 0.27.7, affected by GHSA-g7r4-m6w7-qqqr when its
  development server runs on Windows. `npm audit --omit=dev` is clean and the
  package is not shipped with the extension; a normal `npm audit fix` finds no
  compatible update. — Low impact. — Left open until Vitest accepts a fixed
  `esbuild`; do not use `--force` to hide the compatibility decision.
- **Lodestar task recovery and retirement verbs.** — `reopen_task` returns a task
  stranded in `in_review` or a manual `blocked` hold to claimable `open`, and
  `abandon_task` retires a nonterminal task to terminal `abandoned` (facade + MCP
  tool, regression-tested), making `TaskStatus::Abandoned` reachable and closing
  the retire-a-mis-filed-task gap. — Resolved Jul 2026. Note: the verbs are wired
  in source, but a stale running MCP binary may not expose them until
  rebuilt/restarted (see the stale-binary gap above).
- **`renew_lease` and re-claim disagree on the evidence window.** — Re-claiming
  an expired lease resets `claim_started_at` (a fresh evidence window), but
  `renew_lease` extends the lease without checking expiry and preserves the
  original `claim_started_at`. Two "recover an expired lease" paths therefore
  yield different evidence-window starts. — Low impact on conformance-window
  precision. — Left open; the correct unification (does renewal after lapse open
  a new window?) is a small semantic decision, not yet made.
- **Duplicate `define_goal` title+statement surfaces a raw SQLite error.** — A
  third goal sharing a title and statement collides on the derived
  `goal:{slug}-{hash(statement)}` id and fails with an opaque `UNIQUE
  constraint` error instead of a typed `LodestarError::Invalid`. — Low impact
  (edge case; goals are rarely exact duplicates). — **Fixed Jul 2026:**
  `store::define_goal` pre-checks the derived id and returns a typed
  `LodestarError::Invalid` pointing the author at `supersede_goal`; regression
  test `redefining_an_identical_goal_is_a_typed_error_not_a_raw_sqlite_fault`.
- **A dead defensive guard remains in `record_conformance_and_transition`.** —
  It errors when a predecessor has more than one successor, but
  `task_handoffs.predecessor_id` is the PRIMARY KEY, so the count is always at
  most one and the branch can never fire. — No functional impact; kept as
  documented defense-in-depth rather than removed, since the PK is the real
  guard. — Noted during the Jul 2026 audit.
- **Conformance preflight and completion could disagree on identical evidence.**
  — `check_conformance` returned `aligned` for task `task:aae950aecd78`, then
  `complete_task` immediately reran the optional semantic judge, returned
  `needs_human`, and stranded the task in review despite no evidence or intent
  change. — High impact on verified delivery. — Resolved Jul 2026 by ADR-0025:
  checks now return a durable id + state token, and completion consumes that
  exact audit result without a second model call (task `task:1b5bdafd5e99`).
- **MCP build identity exposes stale running binaries.** Both servers now report
  `serverInfo.version` as `<package-version>+<12-character-git-sha>` during MCP
  initialize. Compare the suffix with `git rev-parse --short=12 HEAD`; a mismatch
  means the server must be rebuilt and restarted before debugging source
  behaviour or relying on newly added tools. The shared Cargo build helper watches
  Git HEAD/ref changes and supports `MINDLEAK_BUILD_SHA` outside a checkout. —
  Resolved Jul 2026.
- **Docs-only (design/ADR) tasks cannot complete via conformance, stranding
  successors.** — A design task produces a docs commit; `complete_task` runs
  ADR-0009 code conformance, which returns `needs_human` ("evidence does not touch
  code bound to the task goal") and parks the task in `in_review` forever. Any
  implementation task chained `blocked_by` a docs-ADR predecessor then never opens
  (`blocked_by` clears only on predecessor `done`), and with no live `reopen_task`
  it cannot be un-gated — clearing the gate via `block_task(id, None)` leaves it
  `blocked` with no predecessor and no path back to `open`. — High impact on the
  design-first workflow. — The intended fix is the accept→decompose bridge
  ([ADR-0023](docs/adr/0023-design-board-accept-bridge.md)): a human `accept_design`
  completes design work without code conformance and decomposes it. Until then, do
  not chain implementation tasks behind docs-ADR design tasks (observed Jul 2026).
- **`next_task` surfaces non-actionable policy tasks.** — A `constraint` goal was
  decomposed into four tasks that merely restate the constraint and can never
  accrue completion evidence; `next_task` (oldest-first) hands one out on every
  call. — Low-medium impact: agents are handed a zombie. — Do not decompose
  `constraint`/`invariant` goals (only `objective` goals decompose); the archive
  verb and the rule are specified in
  [ADR-0019](docs/adr/0019-task-retention-and-board-hygiene.md) (observed Jul 2026).
- **Injected embedders made `MindLeak` non-`Send`.** — Commit `5d52877` added
  `Box<dyn TextEmbedder>` without the thread-safety contract required when the
  maintenance runtime moves `MindLeak` into `std::thread::spawn`. — High impact:
  the workspace build and strict clippy were red. — Resolved Jul 2026 by making
  `TextEmbedder: Send + Sync` and adding compile-time and unit regression
  assertions that `MindLeak: Send` (Lodestar task `task:e0548f57556a`).

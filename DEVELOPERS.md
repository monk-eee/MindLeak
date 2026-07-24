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

The cargo hooks are **scoped and committed-snapshot aware**
(`scripts/cargo-precommit.mjs`): they run `cargo fmt/clippy/test` only for the
crate packages your change touches and, when the live tree could leak another
agent's WIP, materialize the staged or committed tree through a temporary Git
index. No worktree, branch, commit, or shared ref is created (ADR-0032).

Agents use one primary checkout and one shared `fleet/<goal>` branch. Claim work,
then run `node scripts/scoped-commit.mjs -m "<msg>" -- <path>...` to stage and
commit only declared paths (never `git add -A`). Exactly one designated
integrator fetches and reconciles, then publishes the branch's exact `HEAD` with
`node scripts/canonical-push.mjs`. The publisher refuses protected branches,
staged index state, linked worktrees, and remote divergence. If it refuses,
drain active work and reconcile once in the primary checkout; do not cherry-pick
routine work or move `main` beneath dirty files.

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

- **A server restart can strand a legacy base-id claim until lease expiry.** —
  This run claimed work while the configured identity was the legacy `copilot`;
  after the ADR-0030 server restart the process identity became nonce-qualified,
  so owner-guarded lifecycle operations correctly refused the old owner's live
  claim. — Medium migration impact: work is preserved, but the new process must
  wait for lease expiry. — Left explicit: drain live claims before enabling nonce
  identities or add an attributed recovery path for legacy base-id claims.
- **`recall`'s one-off "100% failure" was a missing embedding model, not a bug.** —
  Telemetry showed `recall` as the only tool with an error (1 call / 1 error, 3ms
  fast-fail); the recorded detail was `/v1/embeddings status 404`. Root cause: the
  embedding model (`MINDLEAK_EMBED_MODEL`, default `nomic-embed-text`) was not yet
  pulled into Ollama, so the query-embedding POST 404'd — an environment/config
  issue (ADR-0008: recall is optional and off the deterministic hot path).
  Verified it degrades cleanly (typed `MindLeakError::Http`, no panic/block, no
  hot-path poisoning — 2362 events, only `recall` ever errored) and, once
  `ollama pull nomic-embed-text` was run, recall returns scored results (94ms,
  confirmed live). — Low impact (optional feature, self-announcing at startup and
  documented in QUICKSTART/USAGE). — Resolved: operator remediation already
  documented; contract covered by
  `recall_and_index_degrade_cleanly_when_the_embedder_is_unreachable` (unreachable
  model → error) and `recall_returns_empty_not_error_when_the_index_is_unpopulated`
  (reachable model, empty index → empty, not error); observed on
  `task:2c86cc1f51ea`.
- **Cross-goal bindings on shared *source* files caused false drift — RESOLVED.** —
  Repeated per-task `link_goal_to_code` calls left 10 lodestar /
  mindleak source files each bound to two active goals (e.g. `model.rs`, `lib.rs`,
  `store/coordination.rs`, `facade/conformance.rs`,
  `crates/mindleak-core/src/graph/evidence.rs`), so a commit serving goal A reports
  drift against goal B. — RESOLVED for documentation: goals govern code, not the
  shared prose every task touches, so `evaluate_conformance` now ignores `governed`
  bindings on documentation nodes **at read time** — deleting nothing (commit
  `8ce8516`, which superseded and removed the rejected auto-delete-on-restart
  *clobber* `b55f2a0`; an explicit `forbid_change` lock on a doc is still honoured).
  The one-time clobber had already dropped the 10 documentation bindings (89 → 79)
  before removal; those were benign pollution and are re-linkable. `unlink_goal_from_code`
  + `governing_goals` (commit `6b22bca`) provide an explicit, audited prune path. —
  **RESOLVED Jul 2026 (task:c4bae4cc6ec2)** via human-in-the-loop
  `unlink_goal_from_code` triage: each file's true owner is its plane's objective,
  so the mistaken bindings were the *MindLeak-graph* objective
  (`local-temporal-context-graph`) on the 8 Lodestar source files, and the
  `principled-verified-delivery` **constraint** (a cross-cutting rule, not a
  per-file owner) on `model.rs` and `graph/evidence.rs`. Those 10 bindings were
  dropped (explicit/audited, no auto-delete); each of the 10 files now has exactly
  one governing goal, so honest commits no longer drift. Data-plane only — no code
  change.

- **Blind design promotion could omit governing goals or duplicate existing work
  — FIXED.** — ADR-0024
  was correctly implemented across Lodestar, MindLeak, the extension, evaluation,
  and docs under promoted `task:46dd49254e4c`, but that task belongs only to
  `goal:local-temporal-context-graph`; exact commit evidence produced conformance
  audit `65` with `drift` for the independently governed Intent Plane and
  principled-delivery surfaces. The ADR-0018 audit confirmed the same shape:
  promoted `task:d2900fdfa41b` belongs to the graph goal while its required git
  safety scripts are governed by `goal:principled-verified-delivery`, so exact
  evidence for green commit `321cf17` produced audit `68` with `drift`. ADR-0028
  exposed the second failure mode: deterministic fallback created unblocked
  `task:735e36892ffa` even though release-gated pilot `task:7f5ae1198134` already
  represented the exact work under the Intent Plane objective. — High
  coordination impact: a design could look materialized while bypassing its real
  delivery chain. — Fixed Jul 2026 (`task:53a02c15fa67`): planning is read-only;
  humans review explicit create/link/no-work plans; create may span objectives;
  link reuses authoritative tasks; materialization is atomic/idempotent; repairs
  append attributed revisions and replace only the current projection. The bad
  ADR-0028 task was durably abandoned rather than deleted or relinked by hand.

- **The `evidence_for` → Lodestar conformance seam is sound, but convention-
  sensitive.** — The producer and consumer agree on schema version 1, normalized
  `agent:<id>` observation provenance, successful-execution subset rules, and
  inclusive claim bounds. Executions source `modified` / `failed_on`; commit
  intent nodes source `refactored`, so every changed or failed node names a
  source accepted by `validate_evidence_shape`. This is not a product bug. The
  otherwise-unenforced ingestion convention is pinned by
  `evidence_for_emits_self_consistent_provenance`, which exercises execution,
  failure, and commit evidence and fails if a future ingester emits an unusable
  bundle; `evidence_for_normalizes_prefixed_agent_and_includes_window_boundaries`
  pins agent normalization and inclusive endpoints. — Verified Jul 2026 on
  `task:40c4e757e601`.

- **The real-agent product gate is narrow.** — Three runs per arm on one
  composite typed-session fixture with Copilot CLI 1.0.63 / Haiku 4.5 cross the
  exploration and success thresholds, but do not establish general performance
  across repositories, models, or long-running teams. The two-agent duplicate-
  work mechanism is now covered by ADR-0024's deterministic two-plane overlap
  benchmark, but independent agents' scope accuracy and willingness to heed an
  advisory are not. — Medium impact on claim breadth. — Productization may
  proceed; broader external replications remain required for universal efficacy
  claims.

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
  **Fixed Jul 2026:** resolution now falls to the git *common* dir's parent
  (`git rev-parse --git-common-dir`) so every worktree of a repo shares
  `<repo-root>/.lodestar/spec.db` by default (`LODESTAR_DB` still overrides; cwd
  fallback outside a git repo). Pure `resolve_db_path_from` is unit-tested for all
  three cases.
- **Unit Test MCP 1.3.6 cannot validate this workspace reliably.** — Its Vitest
  discovery finds `src/util.test.ts`, but `run_tests` reports a passing total of
  zero even for that explicit path. On Windows, a backslash Cargo root is
  rejected as `INVALID_ROOT_DIR`; normalizing it to forward slashes runs the
  custom command and surfaces failures, but successful runs still report zero
  tests. Vitest coverage also depends on drive-letter casing: a lowercase `c:`
  root duplicates every covered source as an uppercase `C:` zero-hit shadow,
  falsely reporting 38.64% lines; the canonical uppercase root produces the
  correct unique-file aggregate (89.19% lines / 84.85% branches). — High impact
  on local proof. — Left open in the external adapter; use a canonical uppercase
  Windows drive root for coverage, while CI's test counts remain authoritative.
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
- **`renew_lease` and re-claim now share one evidence-window rule.** — Renewal is
  a heartbeat for a still-live lease and preserves `claim_started_at`; it refuses
  after expiry. A lapsed owner must win `claim_task` again, which resets
  `claim_started_at` and opens a fresh conformance evidence window. The guarded
  single-statement CAS and both paths are regression-tested. — Resolved Jul 2026.
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
- **Docs-only design tasks could not complete via conformance, stranding
  successors — PARTIALLY FIXED.** — A design task produces a docs commit; `complete_task` runs
  ADR-0009 code conformance, which returns `needs_human` ("evidence does not touch
  code bound to the task goal") and parks the task in `in_review` forever. Any
  implementation task chained `blocked_by` a docs-ADR predecessor then never opens
  (`blocked_by` clears only on predecessor `done`), and with no live `reopen_task`
  it cannot be un-gated — clearing the gate via `block_task(id, None)` leaves it
  `blocked` with no predecessor and no path back to `open`. — High impact on the
  design-first workflow. — Fixed for registered *design items* by the accepted
  ADR-0023 Design Board path: a human `accept_design` completes design review
  without code conformance, then a separately reviewed create/link/no-work plan
  maps it to executive work. Blind fallback creation was removed after ADR-0028
  exposed a duplicate-task failure. A docs-only task inside an *objective's*
  task chain (not a registered design item) —
  e.g. the AGENTS.md/README/USAGE/SPEC-INTENT task closing the ADR-0029 advise
  chain — still lands `in_review` via the same `needs_human` verdict. — **Fixed
  Jul 2026:** `resolve_task(task_id, human)` (facade + MCP) is the task-level
  mirror of `accept_design` — it human-accepts an `in_review` task to `done` with
  no code-conformance re-run, opens any blocked successor, and refuses
  self-resolution by the reviewed agent (the worker read from the task's
  conformance evidence). Tests:
  `resolve_task_accepts_an_in_review_task_to_done`,
  `resolve_task_refuses_self_resolution_by_the_reviewed_agent`,
  `resolve_in_review_opens_a_blocked_successor`.
- **`next_task` surfaces non-actionable policy tasks.** — A `constraint` goal was
  decomposed into four tasks that merely restate the constraint and can never
  accrue completion evidence; `next_task` (oldest-first) hands one out on every
  call. — Low-medium impact: agents are handed a zombie. — **Fixed Jul 2026:**
  `decompose_goal` now returns `LodestarError::Invalid` for `constraint`/
  `invariant` goals (only `objective` goals decompose); the four restatement
  tasks were retired with `abandon_task`; regression test
  `constraint_goals_cannot_seed_junk_and_next_task_surfaces_actionable_work`.
- **Injected embedders made `MindLeak` non-`Send`.** — Commit `5d52877` added
  `Box<dyn TextEmbedder>` without the thread-safety contract required when the
  maintenance runtime moves `MindLeak` into `std::thread::spawn`. — High impact:
  the workspace build and strict clippy were red. — Resolved Jul 2026 by making
  `TextEmbedder: Send + Sync` and adding compile-time and unit regression
  assertions that `MindLeak: Send` (Lodestar task `task:e0548f57556a`).

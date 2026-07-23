# Changelog

All notable changes to MindLeak are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project adheres
to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed
- **Lodestar tests are structurally isolated from any ambient local model.** A
  reusable `LlmClient::unreachable()` seam points the optional planning/judging
  model at an unroutable endpoint, so `decompose` and `judge` take their
  deterministic fallback regardless of whatever server a developer is running.
  The core test helper now uses it, and the previously untested `decompose_goal`
  MCP dispatch gains coverage that asserts the single-task fallback offline
  (closes the "Lodestar core tests are not isolated from a running local model"
  known gap).
- **`board` can hide terminal tasks.** The tool and facade gain
  `include_terminal` (default `true`, unchanged behaviour); `false` returns only
  the live/actionable set (open, claimed, in_review, blocked), so completed and
  abandoned work stays durable but drops out of a lean coordination view. Pairs
  with `abandon_task` to keep the board uncluttered without decaying intent
  (ADR-0004: the Intent Plane never expires tasks).
- **Git hooks are scoped and isolation-aware to stop concurrent-agent poisoning.**
  The cargo fmt/clippy/test pre-commit and pre-push hooks now run only for the
  crate packages a change touches, and — on push, or when a foreign untracked
  file sits in an affected crate — validate against a throwaway worktree snapshot
  rather than the shared dirty tree. An unrelated agent's broken crate or
  uncommitted WIP can no longer fail your commit or push (portable runner
  `scripts/cargo-precommit.mjs`; ADR-0018).
- **Two helper scripts for safe concurrent git in a shared tree (ADR-0018).**
  `scripts/scoped-commit.mjs` stages and commits only the paths you declare
  (pathspec; never `git add -A`), so another agent's staged work is never swept
  into your commit; `scripts/isolated-push.mjs` pushes a commit through the hooks
  from a throwaway worktree so another agent's broken WIP cannot poison your
  pre-push validation. A collision harness (`scripts/collision-harness.mjs`,
  `make collision-harness`) proves the no-clobber, independent-commit, and
  honest-merge-conflict properties in a throwaway sandbox repo.

### Added
- **`abandon_task` retires a task to terminal `abandoned`.** `TaskStatus::Abandoned`
  was defined but unreachable — a mis-filed or superseded task could not be retired
  short of `reset_database`. The new store/facade method and MCP tool move a
  nonterminal task (open, in_review, or blocked) to terminal `abandoned`, clearing
  any owner and dependency, while refusing to disturb an active claim (release
  first) or re-retire terminal work. Distinct from `reopen_task` (recover) and
  `reset_database` (wipe). Regression-tested.
- **Inspect a task's conformance evidence from the Intent Board.** Done and
  in-review tasks gain an "Inspect Task Evidence" action that opens the recorded
  evidence — verdict, findings, summary, and the changed/failed node and
  execution/commit ids — as a readable markdown view, resolved read-only from the
  existing `conformance_history` audit (no recomputation, no state change). The
  MindLeak activity-bar icon is now the brain mascot.
- **`conformance_history` resolves a task's durable evidence link.** Completing a
  task records its evidence bundle, verdict, and findings in the append-only
  conformance audit; the new facade method and MCP tool return that chain (each
  record carries a stable `id`, the recorded evidence, `verdict`, `findings`, and
  `checked_at`) so the proof a task is complete is resolvable after the fact
  without duplicating the evidence blob.
- **Telemetry pane in the VS Code extension.** A new sidebar view surfaces a
  real-time effectiveness readout — graph size, tool-call success/error rates,
  average latency, and per-tool metrics — refreshed on an interval
  (`mindleak.telemetryRefreshSecs`, default 3s) while visible. Full live event
  logging is opt-in via a **Live log** toggle (off by default). Numbers are
  derived from the existing `graph_stats` and `telemetry_snapshot` tools; no new
  server surface or hot-path cost.
- **`reopen_task` recovers stranded Lodestar tasks.** A task that landed in
  `in_review` (a drift or needs-human completion outcome) or was manually blocked
  with no predecessor previously had no path back to a claimable state. The new
  facade method and MCP tool return such a task to `open`, while refusing to
  bypass a handoff dependency, disturb an active claim, or revive terminal work.

### Changed
- **Consolidation classifies edge relations instead of always `refactored`.** The
  sleep-phase consolidation prompt now constrains the local model to a closed
  relation vocabulary — `fixed`, `relates_to`, `refactored` — and a new
  `RelationType::Fixed` variant is added. The deterministic layer is authoritative:
  any omitted, unknown, or structural relation the model returns is coerced to
  `refactored`, so fix/bug work and `DECISION:`/`WHY:` rationale links are no
  longer mislabelled as `refactored`.

### Fixed
- **Injected embedding backends remain safe for maintenance worker threads.**
  `TextEmbedder` now requires `Send + Sync`, restoring the workspace build after
  the injectable semantic-recall seam made `MindLeak` non-`Send`. Compile-time
  and unit regression assertions preserve the worker-thread contract.
- **`record_knowledge` now honours a revised half-life.** Re-recording an
  existing statement previously updated weight, evidence, and the revalidation
  clock but silently kept the original `half_life_hours`, so a caller's changed
  revalidation cadence was lost. The `ON CONFLICT` clause now updates it, with a
  regression test.
- **Lodestar goal slugs no longer emit a trailing dash.** `slugify` trimmed
  separators before applying the 48-character cap, so a title whose boundary
  landed on a dash produced a goal id ending in `-`. Truncation now runs before
  trimming, with a regression test.

## [0.1.0-preview.1] - 2026-07-23

### Added
- **Progressive task handoffs** (ADR-0015): `create_task(blocked_by=...)`
  creates an unclaimable successor that opens transactionally only after aligned
  predecessor completion. A deterministic two-connection benchmark demonstrates
  maximum same-file ownership of one versus two concurrent owners for
  independent tasks; advisory symbol leases remain intentionally unshipped.
- **Bounded working-memory tier** (ADR-0017 phase 1): `working_set` returns the
  configured agent's highest active observations, hard-capped at a startup
  `MINDLEAK_WORKING_SET_SIZE` (default 7, bounded 1-32). Sustained observations
  contribute deterministic rehearsal evidence without storing a separate buffer
  or invoking a model.
- **Opt-in autonomous consolidation** (ADR-0017 phase 2): an idle/rate-limited
  worker uses its own file-backed SQLite connection and the existing
  `consolidate_signal` path. A persisted workspace lease prevents duplicate
  manual/idle model spend across processes. Bounded post-model gist/provenance
  writes and unchanged raw candidate acknowledgement commit atomically without
  retaining raw inputs; attempts emit categorized maintenance telemetry and
  shutdown is bounded.
- **Per-project decay policy** (ADR-0014): strict committable
  `.mindleak.toml`, optional `MINDLEAK_CONFIG`, per-relation environment
  overrides, and bounded prune-threshold tuning. `GraphStore` applies the
  startup-resolved policy retroactively at read/prune time without rewriting
  stored edges or effective weights.
- **Productized distribution** (ADR-0016): one-command, JSONC-preserving
  two-plane workspace installer; self-contained platform-targeted VSIX packages;
  versioned native bundles for Windows x64, Linux x64, macOS Intel, and macOS
  Apple Silicon; SHA-256 checksums and signed GitHub provenance attestations;
  and a pinned VS Code 1.93.1 live Extension Host CI smoke.
- **VS Code lifecycle controls and complete health**: complete active-graph
  export, two-plane online backup, modal memory-only reset, and visible memory,
  intent, terminal, and Git health/degraded status.
- **Local data lifecycle** (ADR-0013): shared integrity-checked SQLite online
  backup for both planes; complete active graph JSON export; separately
  confirmed memory (`RESET MINDLEAK`) and durable intent (`RESET LODESTAR`)
  resets; and documented upgrade, rollback, retention, and privacy procedures.
- **Core engine** (`mindleak-core`): SQLite graph + FTS5, exponential half-life
  decay engine, and a registered `effective_weight()` scalar SQL function.
- **Zero-token deterministic ingestion**: `execution` (stack-trace `failed_on`
  parsing), `git` commits (with `DECISION:`/`HACK:` rationale extraction), and
  heuristic `ast` extraction of symbols **and in-file `calls` edges** for 8
  languages.
- **ADR-0006 structural imports, phase 1**: static JavaScript/TypeScript
  `import`/`require` declarations create artifact/package `imports` edges;
  direct calls to named import bindings create cross-file `calls` edges. Both
  participate in artifact-owned reconciliation and relation-directed impact.
  Token-aware extraction filters comments/member calls/basic shadowing, while
  candidate-backed artifact stubs promote across mixed extensions and index
  modules or disappear when their final import is removed.
- **ADR-0006 type hierarchy, phase 2**: simple named JavaScript/TypeScript class
  and interface heritage creates durable `extends`/`implements` edges for local
  and named imported types. Hierarchy participates in incoming impact traversal,
  consumer-first stub promotion, and artifact-owned retraction; the strict truth
  set measures 100% relation and impacted-type precision/recall.
- **ADR-0006 manifest dependencies, phase 3**: direct dependencies in
  `Cargo.toml`, `package.json`, `go.mod`, and `requirements*.txt` create durable
  artifact-owned `depends_on` edges to package nodes. Structured TOML, JSON, and
  PEP 508 parsers preserve renamed/canonical identities; malformed manifests
  fail before reconciliation, preserving the last valid snapshot.
- **MCP server** (`mindleak-mcp`): newline-delimited JSON-RPC 2.0 over stdio
  exposing 21 tools (`graph_multi_hop_query`, `get_impact_radius`,
  `record_architectural_decision`, plus ingestion/snapshot/prune/stats, an
  optional `consolidate_session` helper, `list_agents`, and the optional
  semantic-recall pair `recall`/`index`).
- **Observability, telemetry & network resilience** (ADR-0010): structured
  `tracing` to **stderr** (never stdout, which carries the JSON-RPC channel),
  gated by `MINDLEAK_LOG` / `MINDLEAK_LOG_FORMAT`; a durable, queryable
  `telemetry_events` audit trail recording every tool call (name, outcome,
  latency) surfaced through the `telemetry_snapshot` MCP tool; and a `net` layer
  giving all optional HTTP (embeddings, consolidation, LLM) explicit timeouts,
  bounded retry with backoff, and a per-endpoint **circuit breaker**. Tunable via
  `MINDLEAK_HTTP_TIMEOUT_MS` / `MINDLEAK_HTTP_RETRIES` /
  `MINDLEAK_BREAKER_THRESHOLD` / `MINDLEAK_BREAKER_COOLDOWN_MS`. The deterministic
  path never touches the network; telemetry never touches stdout or graph state.
- **Multi-agent attribution**: set `MINDLEAK_AGENT=<id>` and each ingest/focus
  also records a decay-weighted `agent:<id> --observed--> <node>` edge — shared
  graph, per-agent attention that fades. Roster via `list_agents`.
- **VS Code passive evidence sensors** (ADR-0011): focus boosts a node, save
  ingests structure, shell-integrated terminal start/end events ingest command
  outcomes and workspace mutation evidence, and built-in Git commit events
  ingest commit metadata and changed paths. Output retention is opt-in,
  redacted, and bounded; capture health reports concrete degraded modes.
- **Offline Cytoscape graph visualizer** (vendored, no CDN) with prune/export
  controls.
- **VS Code Intent Board**: a tree view of the Lodestar task board (who owns
  what) plus save-triggered conformance diagnostics (drift/violation surfaced
  inline) via a second `lodestar-mcp` client. Config: `mindleak.lodestarServerPath`,
  `mindleak.lodestarDatabasePath`, `mindleak.conformanceOnSave`.
- **Optional local-LLM consolidation** over the **OpenAI-compatible**
  `/v1/chat/completions` API (Ollama `/v1`, LM Studio, llama.cpp, …), configured
  via `MINDLEAK_LLM_URL` / `MINDLEAK_MODEL` / `MINDLEAK_LLM_API_KEY`; async and
  off the hot path. Both LLM clients (MindLeak + Lodestar) extract the JSON object
  from model output robustly (fence/prose-tolerant), verified end to end against
  `glm4:9b` by `#[ignore]`d live round-trip tests.
- **Optional semantic-recall embedding index** (ADR-0008): an off-hot-path
  vector *lens onto the graph*, complementing decay traversal rather than
  replacing it (ADR-0002). `index` embeds nodes lacking a current vector through
  a local **OpenAI-compatible** `/v1/embeddings` server (Ollama, LM Studio,
  llama.cpp, …), and `recall` returns the nearest node ids by cosine similarity —
  entry points to *seed* `graph_multi_hop_query`, not a substitute for it.
  Embeddings live in a derived, recall-only `embeddings` table and never touch
  the zero-token write path. Configured via `MINDLEAK_EMBED_URL` /
  `MINDLEAK_EMBED_MODEL` / `MINDLEAK_EMBED_API_KEY`; errors cleanly when no
  embedding server is reachable.
- Engineering baseline: pre-commit hooks, rustfmt/clippy/eslint/prettier,
  GitHub Actions CI (Linux + Windows), `.gitattributes`, and the `docs/`
  documentation set.
- **Test coverage pipeline**: CI runs workspace-wide Rust tests under
  `cargo-llvm-cov`, enforces 80% Rust line coverage plus 80% line and branch
  coverage on the extension's unit-testable `util.ts` surface, and uploads both
  LCOV reports for every push and pull request.
- **Tag-driven binary releases**: GitHub Actions gates tags through the full
  repository CI, builds and smoke-checks both MCP servers for Windows x64,
  Linux x64, macOS Intel, and macOS Apple Silicon, then publishes attested
  platform archives with `SHA256SUMS`.
- **Repeatable graph evaluation harness**: a cross-platform MCP/stdio scenario
  records stale-structure and cross-file-impact behavior against a fresh
  temporary database, with machine-readable baseline results, source revision,
  and executable hash. It clears ambient agent attribution and requires a typed
  structural edge before impact can pass.
- **Pinned real-agent outcome gate**: GitHub Copilot CLI 1.0.63 with
  `claude-haiku-4.5` runs no-memory, flat-history, MindLeak, and
  MindLeak+Lodestar arms in randomized fresh workspaces/databases and isolated
  Copilot homes. Across three runs per arm, MindLeak reduced median exploration
  18.2% and reached 66.7% success; MindLeak+Lodestar reached 100% success with
  zero regressions versus 0% for both controls.
- **Lodestar Intent Plane** (`lodestar-core` + `lodestar-mcp`): the durable "spec
  brain" (ADR-0004) — a versioned constitution (goals/constraints/invariants), an
  executive task ledger with an **atomic claim/lease compare-and-swap** for
  collision-free coordination of parallel local agents across worktrees, a
  conformance check (aligned/drift/violation), and **consolidated learned
  knowledge** that is durable-but-revalidated (ADR-0005). A second stdio MCP
  server with 23 tools; optional local SLM for decomposition and semantic
  conformance with deterministic fallbacks; shared `.lodestar/spec.db` (WAL) with
  the constitution exportable to committed markdown.

- **Derived signal-weighted decay** (ADR-0005/0012): every graph read derives a
  bounded half-life multiplier from span-qualified reinforcement, independent
  source diversity, failure/change/success consequence, surprise, structural
  in-degree, and explicit decisions. Effective weight remains derived and the
  multiplier is capped at 8x. `prune_graph` returns near-expiry proven signal
  with provenance and retains expired candidates until optional
  `consolidate_signal` succeeds, then acknowledges the raw evidence.

### Fixed
- Execution ingestion now batches one execution and all artifact edges in a
  single SQLite transaction. The 200-file/8 KiB passive-sensor benchmark moved
  from 296 ms to 28.651 ms p95, below the 50 ms gate.
- The committed dependency graph and source now compile with the declared Rust
  1.75 minimum: `Cargo.lock` uses format 3, parser/TLS transitives are pinned to
  compatible releases, and post-1.75 `Option` helpers use equivalent stable
  expressions.
- The exported `.lodestar/CONSTITUTION.md` is now committable while local
  Lodestar database and lease state remain ignored.
- Extension compiler and VS Code API typings are pinned to supported versions,
  preventing installs from silently advancing beyond the declared toolchain.
- Re-ingesting a source file now atomically replaces its artifact-owned
  structural snapshot, retracting removed symbols and call edges immediately.
- Focusing an entity now updates node attention without reviving the weight or
  decay clock of unrelated failures, modifications, and structural evidence.
- Impact analysis excludes agent observation edges, orphaned removed symbols are
  pruned after historical evidence expires, structural ownership conflicts fail
  atomically, and legacy migrations serialize concurrent openers.

[Unreleased]: https://github.com/monk-eee/MindLeak/compare/v0.1.0-preview.1...HEAD
[0.1.0-preview.1]: https://github.com/monk-eee/MindLeak/releases/tag/v0.1.0-preview.1

# Changelog

All notable changes to MindLeak are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project adheres
to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.1] - 2026-07-24

### Added
- **Two productization decisions make the viability gaps explicit.** ADR-0027
  proposes an extension-led, five-minute first-value workflow over the existing
  portable MCP primitives, without duplicating authoritative state or requiring
  a model. ADR-0028 separates engineering, controlled-efficacy, and external-
  adoption evidence; it defines the privacy-preserving post-v0.1.1 developer
  pilot required before broad product claims or roadmap expansion.
- **Read tools render as rich Markdown in chat while staying machine-parseable.**
  MCP tool results can now carry a chat-facing Markdown rendering in `content`
  *and* the structured JSON in `structuredContent`, so Copilot Chat shows a
  formatted table instead of raw JSON without breaking the programmatic consumers
  (the VS Code extension's panes, agents). The extension's `parseToolResult` now
  prefers `structuredContent`, falling back to today's JSON parse. Wired so far:
  `graph_stats`, `lodestar_stats` (count tables), `next_task` (a task summary
  card), and `telemetry_snapshot` (a per-tool metrics table); other inline read
  tools follow the same `rendered_result` / `rendered` pattern.
- **Pause and resume a claimed task from the Intent Board (ADR-0020).** The board
  now shows an inline pause action on a `claimed` task and a resume action on a
  `paused` one, calling the owner-guarded `pause_task` / `resume_task` tools for the
  task's owner, so work can be parked and picked up again without releasing the
  claim. A pure `leaseActionFor` helper guards a possibly-stale board row (vitest).
- **The graph now self-cleans: the maintenance worker prunes on idle.** Decay hid
  low-weight edges at query time, but the physical rows only left via a manual
  `prune_graph`, so the graph grew unbounded between calls. The idle maintenance
  worker now runs a deterministic, zero-token prune every pass — reaping decayed
  edges and the execution/symbol/stub nodes they orphan ([ADR-0021](docs/adr/0021-node-lifecycle-and-reaping.md))
  — so no manual pruning is needed. On by default (opt out with
  `MINDLEAK_AUTONOMOUS_PRUNE=false`) and independent of the model-dependent
  consolidation/index tier (`MINDLEAK_AUTONOMOUS_CONSOLIDATION`, still opt-in); the
  worker now starts when either is enabled and emits `autonomous_prune` telemetry
  with reap counts.
- **Design items — an accept→promote→decompose bridge for ADRs (ADR-0023).** An ADR
  can be registered as a first-class *design item* that carries the ADR's review
  lifecycle: while `proposed` it is tainted — it lives on a new **Design Board**
  and never appears in `next_task` or the executive board. `accept_design` is the
  attributed human decision *only* — it does **not** run ADR-0009 code conformance
  (a design decision has no code to conform to) and does **not** create tasks,
  resolving the `in_review` dead-end where design/ADR tasks stranded forever; the
  design becomes `accepted` with promotion state `pending`. The separate,
  **idempotent** `promote_design(id, objective_goal_id)` then materialises the work
  in one step: it decomposes the reviewed design into claimable tasks under the
  chosen objective (model-assisted, deterministic single-task fallback), registers
  any mandated constraints/invariants into the constitution, and records durable
  design→goal / design→task provenance links — so a retry returns the same plan
  instead of duplicating it, and a failed decomposition leaves promotion `pending`
  without undoing the acceptance. Keeping the optional model call out of the
  acceptance write means it never serializes unrelated writers. `reject_design` is
  durable and auditable (archive-not-delete). No agent may decide its own design
  (human-in-the-loop). `reconcile_designs` idempotently imports structured
  Proposed/Accepted/Rejected ADR metadata without a model and without creating
  goals or tasks; existing human decisions and promotion state always win.
  `design_board` now returns proposed decisions plus accepted designs awaiting
  promotion or retry. New tools: `register_design`, `reconcile_designs`,
  `design_board`, `accept_design`, `promote_design`, `reject_design`. The VS Code
  sidebar now ships the separate Design Board and workspace ADR sensor: it syncs
  structured ADR metadata on activation/change or manual command, exposes
  attributed accept/reject and objective selection for promotion, keeps failed
  promotion pending/retryable, and renders persisted objective/task/constraint
  provenance for materialized designs.

### Changed
- **The install and usage on-ramp is action-first and easier to follow.** The
  Quickstart now leads with a three-step download-register-restart happy path,
  adds a "confirm it's connected" tool-list check with the one common failure
  cause, and hands the reader a ready-to-paste first prompt that exercises the
  full query -> act -> write-back memory loop. Verifying the release archive is
  presented as a recommended step rather than a prerequisite that blocks first
  value. The README download section leads with the install command and links to
  the walkthrough, and its `.vscode/mcp.json` example now sets `MINDLEAK_AGENT`
  so agent attribution works out of the box.

### Fixed
- **Extension coverage no longer false-fails the 80% gate on Windows.** The V8
  provider's default `all` baseline walk resolved each included file under the
  OS's uppercase drive letter while recording executed coverage under the
  lowercase drive (`file:///c:/...`), so every file was counted twice — a real
  entry plus a phantom 0% one — halving the reported line total (38.6%) and
  failing `test:coverage` on Windows even though real coverage is ~89%. The
  vitest config now reports only executed in-scope files, which every listed file
  is, restoring an accurate cross-platform number.
- **The VS Code MCP client no longer hangs on an unresponsive or missing server.**
  A spawn failure (for example a misconfigured server path) now rejects
  `start()` instead of leaving activation waiting forever; every request carries a
  timeout (default 30s) so a live-but-silent server surfaces an error rather than a
  stuck command; and `stdin` write failures are guarded and logged instead of
  raising an unhandled stream error.
- **The VS Code Intent Board now allocates work instead of merely displaying
  ownership.** Open and expired-claim rows expose claim-for-me and explicit-agent
  allocation with bounded leases; live claims expose owner-explicit renew and
  release actions. Rows show claim windows and live/reclaimable state, and **Next
  Claimable Task** reveals Lodestar's scheduler choice without auto-claiming it.
  CAS loss, stale owner, expiry, and parked ownership remain visible failures —
  the portal does not invent a parallel assignment store or false lock.
- **Intent Board cleanup now handles stale live work, not only completed rows
  (ADR-0019).** Eligible open, in-review, blocked, and expired-claim rows expose
  a confirmed **Retire Task** action that calls `abandon_task`; the task and its
  conformance history remain durable but leave the operational board. Live
  claims and parked ownership remain protected. ADR-0019 now records the shipped
  hide-never-delete model instead of proposing a second archive lifecycle.
- **The VS Code Intent Board no longer grows forever with completed history.**
  It now requests Lodestar's live/actionable view and defensively filters
  terminal `done` / `abandoned` rows before rendering. The durable task and
  conformance records are unchanged and remain available through
  `board(include_terminal=true)`; stale live work can still be deliberately
  retired with `abandon_task`.
- **Expired leases can no longer be renewed with a stale evidence window.**
  `renew_lease` now succeeds only while the caller's lease is still live. After
  expiry, the owner must win `claim_task` again, which resets `claim_started_at`
  just like any other re-claim and gives conformance one unambiguous work window.
- **`decompose_goal` refuses normative goals, so `next_task` stops handing out
  zombie tasks.** Decomposing a `constraint`/`invariant` goal produced tasks that
  merely restate the rule and can never accrue completion evidence; `next_task`
  (oldest-first) then surfaced one on every call, burying real work. Decompose now
  returns a typed `LodestarError::Invalid` for normative goals (only `objective`
  goals decompose), the four pre-existing restatement tasks were retired with
  `abandon_task`, and a regression test proves `next_task` surfaces actionable
  work instead of a restatement.
- **`index` and `consolidate_session` no longer stall on the model network path.**
  The optional embedding `index` pass embedded one node per HTTP request; it now
  batches up to 64 nodes per `/v1/embeddings` call (OpenAI-compatible array
  `input`), turning a full index from hundreds of sequential round trips into a
  handful. And the optional local-model calls (LLM consolidation + embeddings) now
  use a dedicated network policy — a generous timeout (`MINDLEAK_MODEL_TIMEOUT_MS`,
  default 120s) and **no retry**: a slow-but-working generation was classified as a
  transient failure and re-sent up to three times, tripling the wait before failing
  with nothing produced. Re-running `index` / `consolidate` is the retry. The
  deterministic zero-token write/query path is untouched.

### Changed
- **Lodestar's Intent Plane is shared across git worktrees by default.** With no
  `LODESTAR_DB` override, `lodestar-mcp` now resolves the DB to the git *common*
  dir's parent (`git rev-parse --git-common-dir`) — the main repo root — so every
  worktree of a repo opens the same `<repo-root>/.lodestar/spec.db` and coordinates
  through one plane (ADR-0018), instead of each worktree silently getting its own
  `<cwd>/.lodestar/spec.db`. `LODESTAR_DB` still overrides; outside a git repo it
  falls back to the current directory. The resolver is a pure, unit-tested function.
- **Conformance completion now consumes one authoritative checked verdict
  (ADR-0025).** `check_conformance` persists and returns
  `{ id, token, verdict, findings }`; `complete_task` requires that exact object,
  verifies unchanged evidence and relevant goal-binding/knowledge state, and
  transitions without invoking the optional semantic judge again. Identical
  evidence can no longer preflight `aligned` and complete as `needs_human`, and
  completion no longer writes a duplicate audit row.
- **MCP initialize metadata now identifies the exact source build.** MindLeak and
  Lodestar report `serverInfo.version` as
  `<package-version>+<12-character-git-sha>`, so clients can compare it with
  `git rev-parse --short=12 HEAD` and immediately spot a stale running server.
  A shared, dependency-free Cargo build helper resolves the SHA portably and
  supports `MINDLEAK_BUILD_SHA` for builds outside a Git checkout.
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
- **Constitutional governance now has a holistic adoption design (ADR-0026).**
  The Constitution is the policy authority; tests, scanners, and ratchets are
  evidence-producing controls beneath it. The proposed lifecycle handles repos
  with no constitution through deterministic discovery, an opt-in five-principle
  Common Core, versioned extension packs, clause-by-clause adopt/tailor/reject
  review, atomic activation, and explicit expiring waivers. Philosophy is part of
  the architecture: observed habits may propose policy but never become law
  without attributed project adoption.
- **`unlink_goal_from_code` + `governing_goals` keep the ADR-0009 seam honest.**
  `link_goal_to_code` had no inverse, so a goal↔code binding — including one
  mistakenly attached to a shared doc — was permanent. Over a long multi-agent
  session that accumulated cross-goal bindings, and because `evaluate_conformance`
  flags any changed node governed by a *non-task* goal as drift, honest commits
  serving one goal started drifting against goals they do not realise. The new
  `unlink_goal_from_code(goal_id, node_ids)` prunes a stale binding (idempotent; a
  node not bound to the goal is a no-op; unknown goal is a typed `NotFound`), and
  `governing_goals(node_id)` audits which active goals govern a node and how,
  before pruning. Facade + MCP verbs + integration test that reproduces the
  cross-goal drift and shows the same evidence realign to `aligned` after the
  stale binding is removed.
- **Task lifecycle gains `needs_input` and `paused` states (ADR-0020).** Two live
  states reachable only from `claimed` by the owner, both clearing the live lease
  while keeping the owner and `claim_started_at` evidence window — deliberate
  parking, not release or abandonment. `ask_question` parks a task with a durable,
  append-only question for a human; `answer` records the reply and resumes the task
  under the same owner with a fresh lease. `pause_task` / `resume_task` suspend and
  resume owner-held work. A bounded **parking grace** records `parked_at` so a
  vanished owner cannot strand a parked task — after the grace it returns to the
  pool (`claim_task` / `next_task` reclaim it). Abandoning (terminal, non-`done`) a
  predecessor now **cascades**: its blocked successor transactionally reopens, so a
  dead predecessor never deadlocks a handoff chain, while a merely `in_review`
  predecessor keeps the successor correctly gated. New `task_qa` reads the thread.
  Exhaustive `match` on the extended enum; owner-guarded transitions; regression
  tested.
- **The learned-knowledge loop is wired end to end (ADR-0022).** Two seams that
  were dormant are now connected. A `promote_signals` bridge (facade + MCP verb)
  batch-feeds proven-signal candidates — opaque MindLeak node ids plus their
  provenance span — into the existing count+span consolidation gate; it invents no
  new threshold and builds a deterministic templated statement when no local model
  is available, so promotion never depends on an LLM. Conformance now consults
  `active_knowledge` on every check: when a task's changed nodes intersect a proven
  regularity's referenced nodes it attaches an **advisory** finding and may nudge
  an otherwise-`Aligned` verdict to `NeedsHuman` — but is structurally incapable of
  emitting `Violation` (only the Constitution hard-fails), keeping a stale or wrong
  regularity from blocking valid work. Knowledge stays durable-but-revalidated:
  unreconfirmed statements decay out of `active_knowledge` and are pruned.
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
- **`lodestar-mcp` no longer advertises a duplicated `consolidate` tool.** The
  ADR-0022 knowledge-loop change copy-pasted the `consolidate` definition, so
  `tools/list` returned two identical entries and MCP clients saw an ambiguous
  duplicated verb. The duplicate is removed and a `tools_list` regression test
  now asserts every advertised tool name is unique.
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
- **Duplicate `define_goal` returns a typed error instead of a raw SQLite fault.**
  Defining the same title and statement a third time collides on the derived
  `goal:{slug}-{hash(statement)}` id; it previously surfaced an opaque
  `UNIQUE constraint failed` error. `store::define_goal` now pre-checks the
  derived id and returns `LodestarError::Invalid`, pointing the author at
  `supersede_goal`, with a fail-pre/pass-post regression test.

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

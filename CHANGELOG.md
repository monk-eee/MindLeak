# Changelog

All notable changes to MindLeak are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project adheres
to [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **Core engine** (`mindleak-core`): SQLite graph + FTS5, exponential half-life
  decay engine, and a registered `effective_weight()` scalar SQL function.
- **Zero-token deterministic ingestion**: `execution` (stack-trace `failed_on`
  parsing), `git` commits (with `DECISION:`/`HACK:` rationale extraction), and
  heuristic `ast` extraction of symbols **and in-file `calls` edges** for 8
  languages.
- **MCP server** (`mindleak-mcp`): newline-delimited JSON-RPC 2.0 over stdio
  exposing 14 tools (`graph_multi_hop_query`, `get_impact_radius`,
  `record_architectural_decision`, plus ingestion/snapshot/prune/stats, an
  optional `consolidate_session` helper, `list_agents`, and the optional
  semantic-recall pair `recall`/`index`).
- **Multi-agent attribution**: set `MINDLEAK_AGENT=<id>` and each ingest/focus
  also records a decay-weighted `agent:<id> --observed--> <node>` edge — shared
  graph, per-agent attention that fades. Roster via `list_agents`.
- **VS Code extension**: passive editor sensor (focus boosts a node, save ingests
  symbols) and an offline Cytoscape graph visualizer (vendored, no CDN) with
  prune/export controls.
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
- **Repeatable graph evaluation harness**: a cross-platform MCP/stdio scenario
  records stale-structure and cross-file-impact behavior against a fresh
  temporary database, with machine-readable baseline results, source revision,
  and executable hash. It clears ambient agent attribution and requires a typed
  structural edge before impact can pass.
- **Lodestar Intent Plane** (`lodestar-core` + `lodestar-mcp`): the durable "spec
  brain" (ADR-0004) — a versioned constitution (goals/constraints/invariants), an
  executive task ledger with an **atomic claim/lease compare-and-swap** for
  collision-free coordination of parallel local agents across worktrees, a
  conformance check (aligned/drift/violation), and **consolidated learned
  knowledge** that is durable-but-revalidated (ADR-0005). A second stdio MCP
  server with 21 tools; optional local SLM for decomposition and semantic
  conformance with deterministic fallbacks; shared `.lodestar/spec.db` (WAL) with
  the constitution exportable to committed markdown.

- **Signal-weighted decay in MindLeak** (ADR-0005): edges now carry a
  `reinforcement_count` and `first_seen`, and a derived `signal_half_life()`
  extends the half-life of edges reinforced across a span — proven signal resists
  decay while one-offs and same-session spam fade ("decay noise, not signal").
  Wired into every decay query; effective weight stays derived, never stored.

### Design
- **Fuller signal proxies** (ADR-0005): the episodic signal term ships as
  reinforcement-over-span; the further proxies (corroboration/centrality,
  surprise/prediction-error) and consolidating proven episodic clusters into
  Lodestar learned-knowledge remain the next seams.

### Fixed
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

[Unreleased]: https://github.com/monk-eee/MindLeak/commits/main

<p align="center">
  <img src="assets/mindleak_logo.png" alt="MindLeak" width="420">
</p>

# MindLeak

<p align="center">
  <a href="https://github.com/monk-eee/MindLeak/actions/workflows/ci.yml"><img src="https://github.com/monk-eee/MindLeak/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/monk-eee/MindLeak/actions/workflows/release.yml"><img src="https://github.com/monk-eee/MindLeak/actions/workflows/release.yml/badge.svg" alt="Release"></a>
  <a href="https://github.com/monk-eee/MindLeak/releases"><img src="https://img.shields.io/github/v/release/monk-eee/MindLeak?include_prereleases&sort=semver&label=release" alt="Latest release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/rust-1.75%2B-orange.svg" alt="Rust 1.75+">
  <img src="https://img.shields.io/badge/protocol-MCP-8A2BE2.svg" alt="Model Context Protocol">
</p>

**A local, decay-weighted context graph brain for coding agents.**

MindLeak is a **Temporal Context Graph Engine (TCGE)** that turns raw developer
telemetry (terminal runs, git commits, file symbols) into a directional knowledge
graph whose edges **decay on an exponential half-life**, so stale context fades
instead of drowning every query in historical noise.

MindLeak has **two planes**:

- a **Memory plane** — *what happened* and *how the code connects*, that
  **forgets on purpose**. It does structural, multi-hop reasoning
  (*"what breaks if I change this?"*) that similarity search can't, plus semantic
  recall and passive capture of runs / commits / edits. Core:
  [`mindleak-core`](crates/mindleak-core) (SQLite graph + FTS5, decay engine,
  zero-token deterministic ingestion, optional embedding recall + LLM
  consolidation); served by [`mindleak-mcp`](crates/mindleak-mcp) over MCP/stdio.
- an **Intent plane — Lodestar** — the **durable** "spec brain": a versioned
  constitution (goals · constraints · invariants) and a task ledger with
  **atomic claim/lease** coordination, so multiple agents work in parallel
  without clobbering each other, plus evidence-backed conformance checks. Core:
  [`lodestar-core`](crates/lodestar-core); served by
  [`lodestar-mcp`](crates/lodestar-mcp).

A **VS Code extension** ([`editors/vscode`](editors/vscode)) adds passive editor,
shell-execution, and Git sensors, a live Cytoscape graph visualizer, and an
intent board.

It is a from-scratch replacement for flat-log / vector-only agent memory. See
[`docs/SPEC.md`](docs/SPEC.md) for the design and [`docs/`](docs/) for the
architecture and development guides.

> **Zero-token write path.** Ingestion uses pure pattern matching (regex + path +
> exit code) — no LLM tokens. An optional local Ollama model only runs
> asynchronously to consolidate noise into high-level intent nodes.

**New here?** → **[Quickstart](docs/QUICKSTART.md)** (running in minutes) ·
**[Usage guide](docs/USAGE.md)** (how an agent uses the tools).

---

## Where everything is

| I want to… | Go to |
|---|---|
| **Get running fast** | **[docs/QUICKSTART.md](docs/QUICKSTART.md)** |
| **See a normal workflow (scenarios)** | **[docs/WALKTHROUGH.md](docs/WALKTHROUGH.md)** |
| **Learn how to use the tools** | **[docs/USAGE.md](docs/USAGE.md)** |
| Use the VS Code extension | [editors/vscode/README.md](editors/vscode/README.md) |
| Understand the design | [docs/SPEC.md](docs/SPEC.md) · [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) |
| Understand the *intent plane* (spec brain) | [docs/SPEC-INTENT.md](docs/SPEC-INTENT.md) · [docs/SPEC-CONSTITUTION.md](docs/SPEC-CONSTITUTION.md) · [ADR-0004](docs/adr/0004-intent-plane-spec-brain.md) |
| Set up & run locally | [DEVELOPERS.md](DEVELOPERS.md) |
| Contribute a change | [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md) |
| Constraints for AI agents | [AGENTS.md](AGENTS.md) |
| Know *why* it's shaped this way | [RATIONALE.md](RATIONALE.md) · [docs/adr/](docs/adr/) |
| See what changed | [CHANGELOG.md](CHANGELOG.md) |
| Report a vulnerability | [SECURITY.md](SECURITY.md) |
| Know who owns what | [CODEOWNERS](CODEOWNERS) |

---

## Architecture

```mermaid
flowchart TD
  A["Agents · Copilot / Claude / Cursor"]
  subgraph editor["VS Code extension"]
    S["passive sensors<br/>focus · save · terminal · git"]
    V["Cytoscape graph + intent board"]
  end
  subgraph memory["Memory plane · mindleak — decays"]
    I["zero-token ingest<br/>execution · git · ast · imports"]
    G[("SQLite graph + FTS5<br/>decay-weighted edges")]
    D["decay · prune · recall"]
  end
  subgraph intent["Intent plane · lodestar — durable"]
    C["constitution<br/>goals · constraints · invariants"]
    T[("task ledger<br/>atomic claim / lease")]
  end
  O["local model (optional)<br/>consolidation · embeddings"]

  A <-->|MCP| G
  A <-->|MCP| T
  S -->|MCP| I
  I --> G
  D --> G
  G -.async.-> O
  O -.gist · vectors.-> G
  C --- T
  T -. evidence-backed conformance .-> G
```

---

## Build

Requires Rust 1.75+, Node 18+, and VS Code 1.93+ for the extension.

```bash
# Both MCP servers
cargo build --release --locked -p mindleak-mcp -p lodestar-mcp

# Run the test suite
cargo test

# VS Code extension
cd editors/vscode
npm install
npm run compile
```

The server binaries land at `target/release/mindleak-mcp` and
`target/release/lodestar-mcp` (with `.exe` on Windows).

For the full local workflow (lint, format, pre-commit, CI), see
[`DEVELOPERS.md`](DEVELOPERS.md).

---

## Download

Tagged [GitHub Releases](https://github.com/monk-eee/MindLeak/releases) provide
one archive containing both MCP servers for each supported platform:

| Archive suffix | Platform |
|---|---|
| `windows-x64` | Windows x64 |
| `linux-x64` | Linux x64 (glibc) |
| `macos-x64` | macOS Intel |
| `macos-arm64` | macOS Apple Silicon |

Extract the archive, then run `node /path/to/extracted/install.mjs` from your
workspace: the dependency-free Node 20+ installer smoke-tests and registers both
servers without overwriting unrelated MCP entries. Before extracting, verify the
archive against the release's `SHA256SUMS` and its signed GitHub artifact
attestation. Each platform also publishes a targeted VSIX with both native
servers included. The binaries are not OS publisher-signed, so the operating
system may show a warning. Preview versions use tags such as `v0.1.0-preview.1`.

See **[docs/QUICKSTART.md](docs/QUICKSTART.md)** for the full
install-to-first-prompt walkthrough. Measured outcomes, supported
language/platform matrices, and limitations:
[`docs/RELEASE-NOTES.md`](docs/RELEASE-NOTES.md).

---

## Run the MCP server

```bash
# Uses MINDLEAK_DB if set, else <cwd>/.mindleak/graph.db
MINDLEAK_DB="$PWD/.mindleak/graph.db" ./target/release/mindleak-mcp
```

It speaks newline-delimited JSON-RPC 2.0 (MCP) on stdio.

### Register with an MCP client (VS Code / Copilot example)

`.vscode/mcp.json`:

```json
{
  "servers": {
    "mindleak": {
      "command": "${workspaceFolder}/target/release/mindleak-mcp",
      "env": {
        "MINDLEAK_DB": "${workspaceFolder}/.mindleak/graph.db",
        "MINDLEAK_AGENT": "copilot",
        "MINDLEAK_WORKSPACE": "${workspaceFolder}"
      }
    }
  }
}
```

---

## MCP tools

| Tool | Purpose |
|---|---|
| `graph_multi_hop_query` | Traverse N hops from a seed node/phrase, decay-filtered. |
| `get_impact_radius` | Blast radius of editing a file/symbol. |
| `record_architectural_decision` | Persist a decision as a linked intent node. |
| `ingest_execution` | Command + exit code → execution/modified/failed_on edges. |
| `ingest_commit` | Commit → intent node + refactored edges + rationale. |
| `ingest_file` | File → artifact + extracted symbols (`contains`). |
| `forget_file` | Deleted/renamed file → reap its artifact, symbols, and their edges. |
| `reconcile_workspace` | Forget artifacts for files no longer in the workspace set (bulk cleanup). |
| `boost_entity` | Record node focus for recency views without rewriting evidence. |
| `graph_snapshot` | Subgraph for visualization. |
| `prune_graph` | Surface near-expiry proven signal for consolidation, then purge decayed noise and unreferenced stubs. |
| `graph_stats` | Node / active-edge counts. |
| `export_graph` | Complete active graph JSON with fully derived edge weights (not a backup). |
| `backup_database` | Create an integrity-checked online SQLite backup of the memory plane. |
| `reset_database` | Clear regenerable memory only with the exact `RESET MINDLEAK` token. |
| `consolidate_session` | Optional: compress raw logs into one intent node via a local Ollama model. |
| `consolidate_signal` | Optional: consolidate queued proven signal, persist provenance links, then acknowledge raw evidence. |
| `promotion_candidates` | Aggregate expiring proven signal into subject-level candidates for Lodestar `promote_signals` — the deterministic, model-free promotion pass that closes the learned-knowledge loop (ADR-0022). |
| `list_agents` | Roster of agents + their active observation counts (attribution). |
| `working_set` | Current agent's bounded, ranked attentional focus (derived from active observations; default cap 7). |
| `evidence_for` | Bounded, provenance-bearing evidence bundle from an agent's attributed executions/commits in a work window (ADR-0009). |
| `index` | Optional: embed nodes lacking a current vector via a local `/v1/embeddings` server (ADR-0008). |
| `recall` | Optional: nearest node ids by cosine similarity — entry points to *seed* `graph_multi_hop_query`. |
| `telemetry_snapshot` | Observability record (ADR-0010): per-tool lifetime call/error counts, latency, current health (whether each tool's most recent call failed), and recent invocations from the durable audit trail. |

---

## Intent Plane tools (Lodestar)

A second, **durable** MCP server ([`lodestar-mcp`](crates/lodestar-mcp)) — the
"spec brain" that keeps parallel agents aligned to shared intent instead of
diluting it. Register it alongside `mindleak-mcp`; it uses `LODESTAR_DB` (else
`<cwd>/.lodestar/spec.db`), a shared file so local agents and worktrees
coordinate through one plane.

| Tool | Purpose |
|---|---|
| `define_goal` / `supersede_goal` | Write/version the constitution (objective · constraint · invariant). |
| `get_constitution` | The authoritative intent to read **before acting**. |
| `advise` | **Ask before acting** (ADR-0029): given the `artifact:`/`symbol:` ids you intend to change, returns the governing clauses + a proportional disposition (advise / review / block / needs_human). Evidence-free, records nothing, needs no model, never gates a claim. |
| `link_goal_to_code` | Bind a goal to MindLeak `artifact:`/`symbol:` nodes. |
| `unlink_goal_from_code` / `governing_goals` | Prune a stale goal↔code binding, and audit which goals govern a node — keeps conformance honest. |
| `governing_for_task` | The clauses governing a task's linked scope — what the Intent Board surfaces on a claimed task (ADR-0029). |
| `export_constitution` | Render the constitution to committed-friendly markdown. |
| `create_task` / `decompose_goal` | Add claimable work; `create_task(blocked_by=...)` creates a progressive handoff. |
| `next_task` | Suggest the next unblocked, claimable task. |
| `claim_task` / `renew_lease` | **Atomic claim + lease** — renewal is a live heartbeat; after expiry, re-claim opens a fresh evidence window. |
| `complete_task` | Consume the exact authoritative `check_conformance` result (owner-guarded); aligned completes, uncertainty reviews, violation blocks. |
| `release_task` / `block_task` | Return or block work. |
| `reopen_task` | Return a stranded task (in review, or a manual hold) to claimable `open`. |
| `abandon_task` | Retire open/review/blocked or expired-claim work to durable `abandoned`; live and parked ownership stays protected. |
| `ask_question` / `answer` | Park a claimed task in `needs_input` with a durable question; a human `answer` resumes it under the same owner with a fresh lease. |
| `pause_task` / `resume_task` | Owner deliberately parks (`paused`) and resumes work, keeping the claim and evidence window. |
| `task_qa` | The durable, append-only question/answer thread for a task. |
| `board` | Who-owns-what snapshot; the VS Code Intent Board defaults to live/actionable work, while `include_terminal=true` returns durable history. |
| `register_design` / `reconcile_designs` / `design_board` | Register one ADR or idempotently import structured repository ADR metadata; list proposed decisions and accepted designs awaiting promotion, without creating tasks during reconciliation. |
| `accept_design` / `promote_design` / `reject_design` | Human completion path for design work — accept (no code conformance), idempotently materialize reviewed work under an objective, or durably reject; no agent may decide its own design. |
| `check_conformance` | Persist and return `{ id, token, verdict, findings }` for exact checked completion. |
| `conformance_history` | Resolve a task's durable evidence chain — the recorded bundle, verdict, and stable id per check. |
| `consolidate` / `record_knowledge` | Gated promotion of learned regularities. |
| `promote_signals` | Promotion bridge (ADR-0022): batch-feed MindLeak `promotion_candidates` into the gated consolidator; deterministic, model-optional. |
| `active_knowledge` / `reconfirm_knowledge` / `prune_knowledge` | Durable-but-revalidated knowledge. |
| `lodestar_stats` | Goal / task / knowledge counts. |
| `backup_database` | Create an integrity-checked online SQLite backup of the intent plane. |
| `reset_database` | Clear durable intent only with the exact `RESET LODESTAR` token. |

Design: [docs/SPEC-INTENT.md](docs/SPEC-INTENT.md) ·
[docs/SPEC-CONSTITUTION.md](docs/SPEC-CONSTITUTION.md) ·
[ADR-0004](docs/adr/0004-intent-plane-spec-brain.md) ·
[ADR-0005](docs/adr/0005-signal-weighted-decay.md) ·
[ADR-0012](docs/adr/0012-derived-signal-evidence.md) ·
[ADR-0026](docs/adr/0026-constitutional-policy-over-mechanistic-ratchets.md).

Backup, upgrade, rollback, export, reset, and retention guidance:
[docs/DATA-LIFECYCLE.md](docs/DATA-LIFECYCLE.md).

The VS Code sidebar includes a separate **Design Board**. It synchronizes
structured ADR metadata, keeps human review separate from executable task
coordination, supports attributed accept/reject decisions, and exposes
idempotent promotion plus persisted implementation provenance.

---

## Optional local-LLM consolidation

The consolidator speaks the **OpenAI-compatible** `/v1/chat/completions` API, so
it works with Ollama's `/v1` endpoint, LM Studio, llama.cpp's server, or any
compatible host. Point it at your local server:

```bash
export MINDLEAK_LLM_URL="http://localhost:11434/v1"   # Ollama's OpenAI endpoint
export MINDLEAK_MODEL="glm4:9b"                        # or codegeex4:9b, qwen2.5-coder…
# export MINDLEAK_LLM_API_KEY="sk-…"                    # only for hosted servers
```

The consolidator ([`consolidate.rs`](crates/mindleak-core/src/consolidate.rs))
uses a strict JSON `response_format` to compress raw logs into a single `intent`
node via the `consolidate_session` tool. It is optional and never on the hot
path — it errors cleanly when no model is reachable.

Set `MINDLEAK_AUTONOMOUS_CONSOLIDATION=true` to opt in to ADR-0017's idle
worker. It uses a separate SQLite connection, waits 300 idle seconds by default,
attempts at most once per hour, and processes at most 20 expiring candidates.
Manual and idle signal consolidation share one SQLite-backed workspace lease and
attempt interval. Model inference happens before one optimistic transaction that
persists a bounded gist (without raw inputs) and acknowledges only unchanged raw
evidence. Pass outcomes appear in `telemetry_snapshot`; merely configuring a
model never enables autonomous spend.

---

## Layout

```
crates/
  mindleak-core/   memory plane: db · model · decay · graph · ingest · consolidate
  mindleak-mcp/    stdio JSON-RPC MCP server (16 tools)
  lodestar-core/   intent plane: constitution · tasks (claim/lease) · conformance · knowledge
  lodestar-mcp/    stdio JSON-RPC MCP server (25 tools)
editors/
  vscode/          passive sensor + Cytoscape visualizer
docs/              SPEC · SPEC-INTENT · ARCHITECTURE · CONTRIBUTING
```

## License

MIT.

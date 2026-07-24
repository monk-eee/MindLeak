# MindLeak Quickstart

Get an agent talking to a decay-weighted memory graph in a few minutes.

MindLeak ships two local, stdio MCP servers:

- **`mindleak-mcp`** — the memory plane (the context graph).
- **`lodestar-mcp`** — the intent plane (goals + task coordination; optional,
  useful for multiple agents).

Both speak newline-delimited JSON-RPC 2.0 (MCP) on stdin/stdout. Everything is
local: a single SQLite file per plane, no network listener, no cloud.

---

## 1. Install

### Option A — install a release (recommended)

No Rust toolchain and no `PATH` changes — three steps and a restart:

1. **Download** the archive for your OS from
   [GitHub Releases](https://github.com/monk-eee/MindLeak/releases) and extract it
   anywhere.

   | Archive suffix | Platform |
   |---|---|
   | `windows-x64` | Windows x64 |
   | `linux-x64` | Linux x64 (glibc) |
   | `macos-x64` | macOS Intel |
   | `macos-arm64` | macOS Apple Silicon |

2. **Register both servers** into the project you want MindLeak to remember. From
   that project's root, run:

   ```text
   node /path/to/extracted/install.mjs --agent your-name
   ```

   Node.js 20+ is required. The installer smoke-tests both servers, copies them
   to `.mindleak/bin/<version>/`, merges the two registrations into
   `.vscode/mcp.json` (keeping your other servers and comments), writes a Copilot
   CLI config to `.mindleak/copilot-mcp.json`, and adds the local databases to
   `.gitignore`. `--agent` sets a stable identity for attribution and task
   ownership; it defaults to `copilot`.

3. **Restart your MCP client** (VS Code / Copilot, Claude Desktop, or Cursor) so
   it picks up the new registration. For the **Copilot CLI**, start it with
   `copilot --additional-mcp-config @.mindleak/copilot-mcp.json`.

Prefer the editor experience? Each release also ships a platform-targeted VSIX
with both servers bundled. Install it via VS Code's **Extensions: Install from
VSIX** command for the live graph, intent board, passive sensors, and health,
backup, export, and reset controls.

> **No editor? No problem.** VS Code is entirely optional. The installer also
> registers both planes for the **GitHub Copilot CLI** (`.mindleak/copilot-mcp.json`),
> so you can run MindLeak headless — see
> [GitHub Copilot CLI — no editor required](#github-copilot-cli--no-editor-required).

> **Verify first (recommended).** Before extracting, check the archive against the
> release's `SHA256SUMS` and its signed GitHub artifact attestation. The native
> binaries are not yet OS publisher-signed, so Windows/macOS may show a trust
> prompt.

### Option B — build from source

Requires stable Rust 1.75+:

```bash
cargo build --release --locked -p mindleak-mcp -p lodestar-mcp
```

The binaries land at `target/release/mindleak-mcp` and
`target/release/lodestar-mcp` (`.exe` on Windows). Then register them manually —
the next step.

---

## 2. Register manually (only when not using the installer)

The release installer already performs this step. For source builds or other
clients, point the agent's MCP config at each binary.
Use **absolute paths**. Set `MINDLEAK_AGENT` (and `LODESTAR_AGENT`) to a stable
id per agent/session so attribution and task ownership work.

### VS Code / GitHub Copilot — `.vscode/mcp.json`

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
    },
    "lodestar": {
      "command": "${workspaceFolder}/target/release/lodestar-mcp",
      "env": {
        "LODESTAR_DB": "${workspaceFolder}/.lodestar/spec.db",
        "LODESTAR_AGENT": "copilot"
      }
    }
  }
}
```

### Claude Desktop / Cursor — `mcpServers` config

Claude Desktop (`claude_desktop_config.json`) and Cursor (`.cursor/mcp.json`)
use the `mcpServers` key:

```json
{
  "mcpServers": {
    "mindleak": {
      "command": "/abs/path/to/mindleak-mcp",
      "env": {
        "MINDLEAK_DB": "/abs/path/to/project/.mindleak/graph.db",
        "MINDLEAK_AGENT": "claude",
        "MINDLEAK_WORKSPACE": "/abs/path/to/project"
      }
    },
    "lodestar": {
      "command": "/abs/path/to/lodestar-mcp",
      "env": { "LODESTAR_DB": "/abs/path/to/project/.lodestar/spec.db", "LODESTAR_AGENT": "claude" }
    }
  }
}
```

### GitHub Copilot CLI — no editor required

You can run MindLeak entirely from the `copilot` CLI with **no VS Code and no
extension**. If you used the installer (Option A) it already wrote
`.mindleak/copilot-mcp.json` for you; for a source build, create it by hand. The
CLI uses the `mcpServers` key but does **not** expand VS Code's
`${workspaceFolder}`, so its paths must be absolute (ADR-0033). Point the CLI at
the config per run:

```bash
copilot --additional-mcp-config @.mindleak/copilot-mcp.json
```

To make it the machine-wide default instead, merge the same `mcpServers` block
into `~/.copilot/mcp-config.json` (honours `COPILOT_HOME`):

```json
{
  "mcpServers": {
    "mindleak": {
      "command": "/abs/path/to/mindleak-mcp",
      "env": {
        "MINDLEAK_DB": "/abs/path/to/project/.mindleak/graph.db",
        "MINDLEAK_AGENT": "copilot",
        "MINDLEAK_WORKSPACE": "/abs/path/to/project"
      }
    },
    "lodestar": {
      "command": "/abs/path/to/lodestar-mcp",
      "env": { "LODESTAR_DB": "/abs/path/to/project/.lodestar/spec.db", "LODESTAR_AGENT": "copilot" }
    }
  }
}
```

Then restart the client and confirm the connection — the next step.

---

## 3. Confirm it's connected

Restart your MCP client and open its tool list. You should see MindLeak's memory
tools (`get_impact_radius`, `graph_multi_hop_query`, `recall`, the `ingest_*`
family, …) and — if you registered it — Lodestar's intent tools (`define_goal`,
`next_task`, `claim_task`, …). If the tools appear, you're live.

Not seeing them? Diagnostics go to **stderr** (stdout carries only the MCP
protocol), so launch the client from a terminal or set `MINDLEAK_LOG=debug` and
read the startup line. The most common cause is a non-absolute `command` path in
the config.

---

## 4. Your first prompt

The whole point is that your agent *looks before it leaps* and *records what it
learns*. Paste this to your agent to exercise the memory loop in a single turn:

> Before you change `src/auth.ts`, call `get_impact_radius` on it and tell me what
> could break. After we make the change, `ingest_file` the new version and
> `record_architectural_decision` explaining why.

That one request runs the core loop end to end: **query the graph → act → write
back**. From here, [USAGE.md](USAGE.md) walks the full loop, the intent plane, and
every tool.

---

## 5. Smoke-test the protocol (optional)

You can drive a server directly by piping one JSON object per line to its stdin.
This ingests a file, then asks what a change to it would impact:

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"ingest_file","arguments":{"path":"src/auth.ts","content":"export function validateSession(t){return Boolean(t);}"}}}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_impact_radius","arguments":{"target_artifact":"artifact:src/auth.ts"}}}' \
  | MINDLEAK_DB="$PWD/.mindleak/graph.db" ./target/release/mindleak-mcp
```

Startup logs and diagnostics go to **stderr**; stdout carries only JSON-RPC (so
piping stays clean). Set `MINDLEAK_LOG=debug` for more detail, or
`MINDLEAK_LOG=off` for silence.

---

## 6. Optional local models

MindLeak is fully useful with **no model**. Two optional, local, off-hot-path
augmentations light up if you point them at an OpenAI-compatible server (Ollama,
LM Studio, llama.cpp):

```bash
# Consolidation: compress raw logs into one intent node (consolidate_session)
export MINDLEAK_LLM_URL="http://localhost:11434/v1"
export MINDLEAK_MODEL="glm4:9b"

# Semantic recall: embed nodes so `recall` can find them by meaning (ADR-0008)
export MINDLEAK_EMBED_URL="http://localhost:11434/v1"
export MINDLEAK_EMBED_MODEL="nomic-embed-text"    # ollama pull nomic-embed-text
```

Both error cleanly when no server is reachable — they never block the
deterministic path. **Semantic recall additionally needs the embedding model
pulled** (`ollama pull nomic-embed-text`). Until then `recall`/`index` return an
actionable error naming the exact fix, and the server logs `semantic recall:
enabled` or `disabled` at startup so you always know its state. Once the model is
reachable the index refreshes itself on idle (when
`MINDLEAK_AUTONOMOUS_CONSOLIDATION=true`); otherwise run the `index` tool once to
populate vectors.

Autonomous consolidation is disabled by default. To opt in, set
`MINDLEAK_AUTONOMOUS_CONSOLIDATION=true`; the server then uses the same optional
model to distil expiring proven signal after idle. Defaults are 300 idle seconds,
3600 seconds between attempts, and 20 candidates per pass. Attempts are visible
through `telemetry_snapshot`.

---

## 7. Next steps

- **[WALKTHROUGH.md](WALKTHROUGH.md)** — a normal day in four end-to-end
  scenarios (look-before-you-leap, ADR-to-tasks, two agents splitting a goal,
  passive capture), with the VS Code panels shown.
- **[USAGE.md](USAGE.md)** — how an agent actually uses the tools (the memory
  loop, the intent plane, the full config reference).
- **[DATA-LIFECYCLE.md](DATA-LIFECYCLE.md)** — backup, upgrade/rollback, export,
  reset, retention, and privacy.
- **[RELEASE-NOTES.md](RELEASE-NOTES.md)** — measured outcomes, supported
  platform/language matrix, and limitations.
- **[SPEC.md](SPEC.md)** / **[ARCHITECTURE.md](ARCHITECTURE.md)** — the design.
- **[../DEVELOPERS.md](../DEVELOPERS.md)** — building, testing, and contributing.

# MindLeak Quickstart

Get an agent talking to a decay-weighted memory graph in a few minutes.

MindLeak ships two local, stdio MCP servers:

- **`mindleak-mcp`** — the memory plane (the context graph).
- **`lodestar-mcp`** — the intent plane (goals + task coordination; optional,
  useful for multiple agents).

Both speak newline-delimited JSON-RPC 2.0 (MCP) on stdin/stdout. Everything is
local: a single SQLite file per plane, no network listener, no cloud.

---

## 1. Get the binaries

### Option A — download a release (fastest)

Grab the archive for your platform from
[GitHub Releases](https://github.com/monk-eee/MindLeak/releases), verify it
against the release's `SHA256SUMS`, and extract `mindleak-mcp` and `lodestar-mcp`.

| Archive suffix | Platform |
|---|---|
| `windows-x64` | Windows x64 |
| `linux-x64` | Linux x64 (glibc) |
| `macos-x64` | macOS Intel |
| `macos-arm64` | macOS Apple Silicon |

On macOS/Linux, mark them executable with your platform's permission tooling.
Preview builds are checksummed but not code-signed, so the OS may warn.

### Option B — build from source

Requires a recent stable Rust (1.75+).

```bash
cargo build --release --locked -p mindleak-mcp -p lodestar-mcp
```

The binaries land at `target/release/mindleak-mcp` and
`target/release/lodestar-mcp` (`.exe` on Windows).

---

## 2. Register the servers with your agent

MindLeak is an MCP server, so you point your agent's MCP config at the binary.
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
        "MINDLEAK_AGENT": "copilot"
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
      "env": { "MINDLEAK_DB": "/abs/path/to/project/.mindleak/graph.db", "MINDLEAK_AGENT": "claude" }
    },
    "lodestar": {
      "command": "/abs/path/to/lodestar-mcp",
      "env": { "LODESTAR_DB": "/abs/path/to/project/.lodestar/spec.db", "LODESTAR_AGENT": "claude" }
    }
  }
}
```

Restart the client; it launches the servers and lists their tools. If a tool
list appears, you're connected.

---

## 3. Smoke-test it (optional)

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

## 4. Optional local models

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
deterministic path.

---

## 5. Next steps

- **[USAGE.md](USAGE.md)** — how an agent actually uses the tools (the memory
  loop, the intent plane, the full config reference).
- **[DATA-LIFECYCLE.md](DATA-LIFECYCLE.md)** — backup, upgrade/rollback, export,
  reset, retention, and privacy.
- **[SPEC.md](SPEC.md)** / **[ARCHITECTURE.md](ARCHITECTURE.md)** — the design.
- **[../DEVELOPERS.md](../DEVELOPERS.md)** — building, testing, and contributing.

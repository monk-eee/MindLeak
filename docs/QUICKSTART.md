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

   | Archive suffix | Platform            |
   | -------------- | ------------------- |
   | `windows-x64`  | Windows x64         |
   | `linux-x64`    | Linux x64 (glibc)   |
   | `macos-x64`    | macOS Intel         |
   | `macos-arm64`  | macOS Apple Silicon |

2. **Register both servers** into the project you want MindLeak to remember. From
   that project's root, run:

   ```text
   node /path/to/extracted/install.mjs --agent your-name
   ```

   Node.js 20+ is required. The installer smoke-tests both servers, copies them
   to `.mindleak/bin/<version>/`, merges the two registrations into
   `.vscode/mcp.json` (keeping your other servers and comments), writes a Copilot
   CLI config to `.mindleak/copilot-mcp.json`, installs the project-scoped
   `/mindleak` agent skill under `.github/skills/mindleak`, and adds the local
   databases to `.gitignore`. Reinstalling updates untouched skill files and
   preserves local edits. `--agent` sets a stable label for attribution and task
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

### VS Code Agents window (Preview)

The release VSIX can provide both local MCP servers to the Agents window. Enable
the extension there in your user settings and reload the window:

```json
{
  "extensions.supportAgentsWindow": {
    "monk-eee.mindleak": true
  }
}
```

Start a session against a **local folder** and select **Copilot CLI**. Either
folder or worktree isolation works. Confirm `mindleak` and `lodestar` under the
Agents window's **Customizations > MCP Servers**, then run `/mindleak verify`.
The two planes must return the same session identity and `repository_id`.

This support is local, not universal: Copilot Cloud cannot reach the stdio
processes or SQLite databases on your machine. For SSH/dev-tunnel sessions,
install MindLeak on that remote host. Quick chats have no repository scope and
should not create repository evidence or own Lodestar tasks. See the full
[Agents-window support matrix](USAGE.md#vs-code-agents-window-preview).

### Option B — build from source

Requires stable Rust 1.85+:

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
Use **absolute paths**. `MINDLEAK_AGENT` and `LODESTAR_AGENT` are matching,
human-readable base labels, not identities. Each client session mints one
128-bit token, calls `open_session` on both planes, and reuses that token on
identity-bearing calls. The VS Code extension performs this handshake itself.

### VS Code / GitHub Copilot — `.vscode/mcp.json`

```json
{
  "servers": {
    "mindleak": {
      "command": "${workspaceFolder}/target/release/mindleak-mcp",
      "env": {
        "MINDLEAK_AGENT": "copilot",
        "MINDLEAK_WORKSPACE": "${workspaceFolder}"
      }
    },
    "lodestar": {
      "command": "${workspaceFolder}/target/release/lodestar-mcp",
      "env": {
        "LODESTAR_AGENT": "copilot",
        "MINDLEAK_WORKSPACE": "${workspaceFolder}"
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
        "MINDLEAK_AGENT": "claude",
        "MINDLEAK_WORKSPACE": "/abs/path/to/project"
      }
    },
    "lodestar": {
      "command": "/abs/path/to/lodestar-mcp",
      "env": {
        "LODESTAR_AGENT": "claude",
        "MINDLEAK_WORKSPACE": "/abs/path/to/project"
      }
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
        "MINDLEAK_AGENT": "copilot",
        "MINDLEAK_WORKSPACE": "/abs/path/to/project"
      }
    },
    "lodestar": {
      "command": "/abs/path/to/lodestar-mcp",
      "env": {
        "LODESTAR_AGENT": "copilot",
        "MINDLEAK_WORKSPACE": "/abs/path/to/project"
      }
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
`task_query`, `task_claim`, …). A headless client must call `open_session` before
using identity-bearing tools. Today that is one call to each plane with the same
session token; both replies must resolve the same agent id. ADR-0097 proposes a
first-party coordinator that makes this one physical call. If the tools appear,
you're live.

On a fresh repository Lodestar `open_session` returns `get_started` instead of
leaving `active_goals: 0` unexplained. Create the first objective with
`constitution_define(action="goal")`, or import caller-supplied accepted ADR
records with `constitution_define(action="import")`; then create and claim work
under the returned goal id. Lodestar does not infer or activate policy by parsing
Markdown.

Call `storage_status` on both planes. Their `repository_id` values must match;
the database paths should share one `repositories/<id>/` directory. Every linked
worktree of this clone resolves the same pair automatically. Independent clones
intentionally receive different ids.

Not seeing them? Diagnostics go to **stderr** (stdout carries only the MCP
protocol), so launch the client from a terminal or set `MINDLEAK_LOG=debug` and
read the startup line. The most common cause is a non-absolute `command` path in
the config.

---

## 4. Your first prompt

The installer adds a project-scoped skill so agents can set up, verify, and use
both planes without making you remember the tool sequence. Start with:

> `/mindleak verify`

Then exercise the memory loop in a real change:

> `/mindleak work on src/auth.ts: tell me what could break before changing it,
make the requested change, validate it, and write back useful evidence.`

The skill opens one identity across both planes, checks live claims and recent
footprints, consults governance, grounds the edit, renews task leases when
needed, and completes with evidence. From here, [USAGE.md](USAGE.md) walks the
full loop, the intent plane, and every tool.

---

## 5. Smoke-test the protocol (optional)

You can drive a server directly by piping one JSON object per line to its stdin.
This ingests a file, then asks what a change to it would impact:

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"open_session","arguments":{"session_id":"00112233445566778899aabbccddeeff"}}}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ingest_file","arguments":{"session_id":"00112233445566778899aabbccddeeff","path":"src/auth.ts","content":"export function validateSession(t){return Boolean(t);}"}}}' \
  '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"get_impact_radius","arguments":{"target_artifact":"artifact:src/auth.ts"}}}' \
  | ./target/release/mindleak-mcp
```

Startup logs and diagnostics go to **stderr**; stdout carries only JSON-RPC (so
piping stays clean). Set `MINDLEAK_LOG=debug` for more detail, or
`MINDLEAK_LOG=off` for silence.

---

## 6. Optional model augmentations

MindLeak is fully useful with **no model**. Two optional, off-hot-path
augmentations use the OpenAI-compatible endpoint you configure. The defaults and
examples below are local (Ollama, LM Studio, or llama.cpp); a compatible hosted
endpoint also works with its API key and deliberately sends the bounded request
to that service.

```bash
# Consolidation: compress raw logs into one intent node (consolidate_session)
export MINDLEAK_LLM_URL="http://localhost:11434/v1"
export MINDLEAK_MODEL="glm4:9b"

# Semantic recall: embed nodes so `recall` can find them by meaning (ADR-0008)
export MINDLEAK_EMBED_URL="http://localhost:11434/v1"
export MINDLEAK_EMBED_MODEL="nomic-embed-text"    # ollama pull nomic-embed-text
```

Neither augmentation blocks the deterministic path when its server is
unreachable. **Semantic recall additionally needs the embedding model pulled**
(`ollama pull nomic-embed-text`). Until then `index` returns an actionable error,
while `recall` returns deterministic graph/FTS results plus the exact Ollama or
alternate-endpoint remedy. Autonomous indexing records an unavailable optional
endpoint once as `skipped`/degraded rather than a deterministic-path error,
silences identical retries, and uses exponential backoff to one hour on the
default cadence (never shorter than an explicitly configured interval). Idle
`Ok(0)` passes are silent and recovery is recorded once. Once reachable, the configured
300-second cadence resumes independently of autonomous consolidation; set
`MINDLEAK_AUTONOMOUS_INDEX=false` to disable those attempts.

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

# mindleak-client

A typed Node.js client for MindLeak's two MCP servers (`mindleak-mcp`,
`lodestar-mcp`) — the packaged reference implementation ADR-0103 decided
should exist instead of every consumer hand-rolling the stdio JSON-RPC
protocol (as the VS Code extension's `mcpClient.ts` and CompLeak's
`lib/mindleak-client.mjs` each did independently before this existed).

This package adds no new protocol surface. It wraps exactly what
`mindleak-mcp`/`lodestar-mcp` already expose over stdio, documented in
[`docs/TOOLS.md`](../../../docs/TOOLS.md).

## Install

Not yet published; consume from a workspace/path dependency until it is.

## Usage

```ts
import { MindLeakClient } from "mindleak-client";
import { randomBytes } from "node:crypto";

const client = new MindLeakClient("mindleak-mcp"); // or an absolute path
await client.connect();
await client.openSession(randomBytes(16).toString("hex"));

const tools = await client.listTools();
const recent = await client.graph.recall({ query: "decay policy" });
const board = await client.tasks.board();

client.close();
```

Point at a specific binary via the command you construct `MindLeakClient`
with; this package never bundles or spawns a server on its own (ADR-0103
decision 6). Resolve one the same way any consumer does: an env var
override, falling back to `~/.mindleak/bin/`.

## Shape

- `client.callTool(name, args)` — the generic escape hatch every typed method
  is a thin wrapper over. Prefer the typed methods; drop to this only for a
  tool with no service method yet.
- `client.knowledge` — `record`, `active`, `retire`, `reconfirm` (ADR-0022).
- `client.tasks` — `create`, `claim`, `renew`, `release`, `complete`, `block`,
  `board`, `overlap`, `next` (ADR-0015/0020/0048).
- `client.evidence` — `evidenceFor`, `checkConformance`, `history`
  (ADR-0009/0025).
- `client.graph` — `recall`, `multiHop`, `impactRadius`, `checkOverlap`,
  `workingSet`, `stats`.

`McpConnection` (in `src/protocol.ts`) is the low-level newline-delimited
JSON-RPC framing, kept separate from process-spawning so it is unit-testable
with plain in-memory streams — see `test/protocol.test.ts`.

## Develop

```bash
npm install
npm run build
npm test
node examples/check-connection.mjs   # live smoke test against a real server
```

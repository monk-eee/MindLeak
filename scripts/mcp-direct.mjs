// Drives Lodestar/MindLeak tool calls directly against the built release
// binaries over the same newline-delimited JSON-RPC stdio
// scripts/canonical-push.mjs already speaks, bypassing a persistent editor
// MCP connection entirely.
//
// Exists because that persistent connection can break for one client session
// and not recover on its own, even across a window reload -- see
// gaps.d/mcp-server-processes-accumulate-per-editor-window.md -- while the
// underlying binaries and their SQLite-backed state are otherwise completely
// unaffected: canonical-push.mjs already drives a fresh instance of them over
// stdio on every publish, regardless of whether any editor's own MCP
// connection is healthy. This is that same mechanism made runnable on its
// own, so a session that has lost its editor connection is not also
// forced to skip claiming, checking overlap, or recording evidence.
//
// A batch of calls MUST be one invocation: each server process is stateless
// across invocations (session state lives only in that one process's
// memory), so `open_session` and everything that depends on it have to run
// in the same call to see it.
//
// Usage:
//   node scripts/mcp-direct.mjs <lodestar|mindleak> <calls.json>
// where calls.json is `[{"name": "...", "arguments": {...}}, ...]`, the same
// shape scripts/claim-gate.mjs's callTools already takes.

import { readFileSync } from "node:fs";

import { callTools, resolveServer } from "./claim-gate.mjs";

/** Resolve the plane's binary once, then forward every call to it as one batch. */
export function runDirectCalls(
  plane,
  calls,
  {
    repoRoot = process.cwd(),
    resolveServerFn = resolveServer,
    callToolsFn = callTools,
  } = {},
) {
  const binary = resolveServerFn(repoRoot, plane);
  if (!binary) {
    throw new Error(`no ${plane} binary found under ${repoRoot}/target`);
  }
  return callToolsFn(binary, repoRoot, calls, 16 * 1024 * 1024);
}

if (import.meta.filename === process.argv[1]) {
  const [, , plane, callsFile] = process.argv;
  if (!plane || !callsFile) {
    console.error(
      "usage: node scripts/mcp-direct.mjs <lodestar|mindleak> <calls.json>",
    );
    process.exit(1);
  }
  const calls = JSON.parse(readFileSync(callsFile, "utf8"));
  const results = runDirectCalls(plane, calls);
  console.log(JSON.stringify(results, null, 2));
}

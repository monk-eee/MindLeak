#!/usr/bin/env node
// Measure the advertised MCP tool surface: how many tools each server offers,
// and what `tools/list` costs to load.
//
// Every agent that connects loads the whole surface before asking its first
// question, so the surface is a per-session tax paid in every worktree of a
// fleet. Nothing in the repository treated that as a cost that must be paid
// down, so it only ever grew — the counter-pressure ADR-0059 identifies as the
// actual missing piece. This is the number that supplies it. Measured between
// the ADR being written and this script landing, `lodestar-mcp` went from 89
// tools to 90: one day, one tool, nobody deciding to grow it.
//
// What is measured, and why it means what it appears to mean:
//   - The servers are *asked*, over the same newline-delimited JSON-RPC stdio a
//     client uses. Counting `json!({...})` blocks in the Rust source would
//     measure the code rather than the surface, and the two are free to drift.
//   - The unit is the compact JSON of the `tools` array — what actually crosses
//     the wire into a context window. Pretty-printing is a client's choice and
//     would inflate the number by about 37% without anyone advertising more.
//   - Tokens are bytes/4, and are reported as approximate because they are. The
//     exact number is the tool count; the token figure is the cost that count
//     implies, and pretending to tokeniser precision here would be false rigour.
//   - A missing binary refuses. Measuring one plane and reporting the total
//     would show the surface halving, which reads as a triumph and is a build
//     error — the one failure mode a budget must not have.
//
// Measuring never touches live state: each server is pointed at a throwaway
// database in a temp directory.
//
// Cross-platform, dependency-free Node (toolchain rule). Usage:
//   node scripts/measure-tool-surface.mjs [--json] [--out <path>]
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { resolveServer } from "./claim-gate.mjs";

/** Bytes per token. An approximation, and labelled as one everywhere it shows. */
export const BYTES_PER_TOKEN = 4;

export const PLANES = ["mindleak", "lodestar"];

/**
 * What one advertised surface costs.
 *
 * Compact rather than pretty on purpose: the array as serialised for the wire
 * is the thing a client pays for.
 */
export function surfaceOf(tools) {
  const bytes = Buffer.byteLength(JSON.stringify(tools), "utf8");
  return {
    tools: tools.length,
    bytes,
    approx_tokens: Math.round(bytes / BYTES_PER_TOKEN),
  };
}

/** The sum of several measured surfaces — the number a session actually loads. */
export function combine(surfaces) {
  const bytes = surfaces.reduce((total, s) => total + s.bytes, 0);
  return {
    tools: surfaces.reduce((total, s) => total + s.tools, 0),
    bytes,
    approx_tokens: Math.round(bytes / BYTES_PER_TOKEN),
  };
}

/**
 * The `tools` array one server advertises, read from its stdio.
 *
 * `initialize` first because the servers answer in protocol order, and the
 * environment is overridden so a measurement can never open the real graph or
 * ledger: this runs on a developer's machine, beside live databases.
 */
export function listTools(binary, cwd) {
  const requests = [
    {
      jsonrpc: "2.0",
      id: 0,
      method: "initialize",
      params: {
        protocolVersion: "2024-11-05",
        capabilities: {},
        clientInfo: { name: "measure-tool-surface", version: "1" },
      },
    },
    { jsonrpc: "2.0", id: 1, method: "tools/list", params: {} },
  ];
  const raw = execFileSync(binary, [], {
    cwd,
    encoding: "utf8",
    input: requests.map((request) => JSON.stringify(request)).join("\n") + "\n",
    stdio: ["pipe", "pipe", "pipe"],
    timeout: 30_000,
    env: {
      ...process.env,
      MINDLEAK_DB: path.join(cwd, "graph.db"),
      MINDLEAK_WORKSPACE: cwd,
      LODESTAR_DB: path.join(cwd, "spec.db"),
      LODESTAR_WORKSPACE: cwd,
    },
  });
  return parseToolList(raw, path.basename(binary));
}

/** The advertised tools in a server's newline-delimited JSON-RPC output. */
export function parseToolList(raw, label = "server") {
  for (const line of raw.split(/\r?\n/)) {
    if (!line.trim()) continue;
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      continue;
    }
    if (message.id !== 1) continue;
    if (Array.isArray(message.result?.tools)) return message.result.tools;
    throw new Error(`${label} answered tools/list without a tools array`);
  }
  throw new Error(`${label} never answered tools/list`);
}

/**
 * Measure every plane, or refuse.
 *
 * `read` is injected so the tests can measure a surface without a Rust build;
 * the alternative is a test suite that can only assert the refusal path, which
 * would leave the measurement itself unmeasured.
 */
export function measure(
  repoRoot,
  { read = listTools, locate = resolveServer } = {},
) {
  const scratch = mkdtempSync(path.join(tmpdir(), "tool-surface-"));
  try {
    const planes = PLANES.map((plane) => {
      const binary = locate(repoRoot, plane);
      if (!binary) {
        throw new Error(
          `no ${plane}-mcp binary: build it with \`cargo build\`, or point at one with ` +
            `${plane.toUpperCase()}_MCP_BIN.\n` +
            "  This refuses rather than measuring what it can find: a surface reported without one " +
            "of its servers halves the number, which reads as progress and is a missing build.",
        );
      }
      return { plane, binary, ...surfaceOf(read(binary, scratch)) };
    });
    return { planes, combined: combine(planes) };
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

const kb = (bytes) => `${(bytes / 1024).toFixed(1)} KB`;

export function report({ planes, combined }) {
  const rows = planes.map(
    (p) =>
      `  ${`${p.plane}-mcp`.padEnd(14)}${String(p.tools).padStart(4)} tools` +
      `${kb(p.bytes).padStart(11)}  ~${p.approx_tokens.toLocaleString("en-US")} tokens`,
  );
  return [
    ...rows,
    `  ${"combined".padEnd(14)}${String(combined.tools).padStart(4)} tools` +
      `${kb(combined.bytes).padStart(11)}  ~${combined.approx_tokens.toLocaleString("en-US")} tokens`,
    "",
    "Loaded in full by every agent, in every session, before the first question.",
    "Token figures are bytes/4 — approximate by construction; the tool count is exact.",
  ].join("\n");
}

const thisFile = fileURLToPath(import.meta.url);
if (process.argv[1] && path.resolve(process.argv[1]) === thisFile) {
  const repoRoot = execFileSync("git", ["rev-parse", "--show-toplevel"], {
    encoding: "utf8",
  }).trim();

  let measured;
  try {
    measured = measure(repoRoot);
  } catch (error) {
    console.error(`measure-tool-surface: ${error.message}`);
    process.exit(1);
  }

  const artifact = {
    evaluated_at: Math.floor(Date.now() / 1000),
    method: {
      source: "tools/list over MCP stdio",
      unit: "compact JSON of the tools array",
      bytes_per_token: BYTES_PER_TOKEN,
      tokens_are_approximate: true,
    },
    planes: measured.planes.map(({ plane, tools, bytes, approx_tokens }) => ({
      plane,
      tools,
      bytes,
      approx_tokens,
    })),
    combined: measured.combined,
  };

  const outFlag = process.argv.indexOf("--out");
  if (outFlag !== -1) {
    const target = process.argv[outFlag + 1];
    if (!target) {
      console.error("measure-tool-surface: --out needs a path");
      process.exit(1);
    }
    writeFileSync(target, `${JSON.stringify(artifact, null, 2)}\n`, "utf8");
    console.log(`measure-tool-surface: wrote ${target}`);
  }

  if (process.argv.includes("--json")) {
    console.log(JSON.stringify(artifact, null, 2));
  } else if (outFlag === -1) {
    console.log(report(measured));
  }
}

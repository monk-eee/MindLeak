// Tests for the tool surface measurement. Run with: make script-test
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync } from "node:fs";

import {
  BYTES_PER_TOKEN,
  combine,
  measure,
  parseToolList,
  surfaceOf,
} from "./measure-tool-surface.mjs";

const tool = (name, padding = 0) => ({
  name,
  description: "x".repeat(padding),
  inputSchema: { type: "object" },
});

const rpc = (id, result) =>
  `${JSON.stringify({ jsonrpc: "2.0", id, result })}\n`;

// Pretty-printing is a client's choice and inflates the payload by roughly a
// third. Measuring it would report a cost nobody advertises.
test("a surface is measured compact, as it crosses the wire", () => {
  const tools = [tool("a", 100), tool("b", 100)];
  assert.equal(
    surfaceOf(tools).bytes,
    Buffer.byteLength(JSON.stringify(tools)),
  );
  assert.ok(
    surfaceOf(tools).bytes < Buffer.byteLength(JSON.stringify(tools, null, 2)),
  );
});

test("tokens are bytes over the declared approximation", () => {
  const measured = surfaceOf([tool("a", 400)]);
  assert.equal(
    measured.approx_tokens,
    Math.round(measured.bytes / BYTES_PER_TOKEN),
  );
});

// Summing rounded per-plane token figures compounds each rounding error into
// the combined number, which is the one a budget is set against.
test("the combined token figure comes from the combined bytes", () => {
  const left = { tools: 1, bytes: 10, approx_tokens: 3 };
  const right = { tools: 2, bytes: 10, approx_tokens: 3 };
  const combined = combine([left, right]);
  assert.equal(combined.tools, 3);
  assert.equal(combined.bytes, 20);
  assert.equal(combined.approx_tokens, 5);
});

test("the tools array is read from the tools/list response", () => {
  const raw =
    rpc(0, { protocolVersion: "2024-11-05" }) +
    rpc(1, { tools: [tool("recall")] });
  assert.deepEqual(
    parseToolList(raw).map((t) => t.name),
    ["recall"],
  );
});

// A server that logs a plain line to stdout must not be mistaken for one that
// answered nothing.
test("non-JSON output is stepped over, not treated as an answer", () => {
  const raw = `ready\n${rpc(1, { tools: [tool("recall")] })}`;
  assert.equal(parseToolList(raw).length, 1);
});

test("a server that never answers tools/list is an error, not an empty surface", () => {
  assert.throws(
    () => parseToolList(rpc(0, {}), "lodestar-mcp"),
    /lodestar-mcp never answered tools\/list/,
  );
});

test("an answer without a tools array is an error", () => {
  assert.throws(
    () => parseToolList(rpc(1, {}), "mindleak-mcp"),
    /without a tools array/,
  );
});

test("both planes are measured and summed", () => {
  const surfaces = {
    "mindleak-mcp": [tool("recall"), tool("index")],
    "lodestar-mcp": [tool("next_task")],
  };
  const measured = measure("/repo", {
    locate: (_root, plane) => `${plane}-mcp`,
    read: (binary) => surfaces[binary],
  });
  assert.deepEqual(
    measured.planes.map((p) => [p.plane, p.tools]),
    [
      ["mindleak", 2],
      ["lodestar", 1],
    ],
  );
  assert.equal(measured.combined.tools, 3);
});

// The failure a budget must not have: half the surface reported as the whole
// surface reads as a win, and is a missing build.
test("a missing binary refuses instead of halving the number", () => {
  assert.throws(
    () =>
      measure("/repo", {
        locate: (_root, plane) =>
          plane === "lodestar" ? null : "mindleak-mcp",
        read: () => [tool("recall")],
      }),
    /no lodestar-mcp binary/,
  );
});

test("the throwaway workspace is removed even when a plane refuses", () => {
  let scratch;
  assert.throws(() =>
    measure("/repo", {
      locate: (_root, plane) => (plane === "lodestar" ? null : "mindleak-mcp"),
      read: (_binary, cwd) => {
        scratch = cwd;
        return [tool("recall")];
      },
    }),
  );
  assert.ok(
    scratch,
    "the first plane should have been measured in a scratch directory",
  );
  assert.equal(existsSync(scratch), false);
});

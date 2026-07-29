// Tests for the module-length ratchet reporter. Run with: make script-test
//
// Only the refusal paths are exercised. A test that reached the Intent Plane
// would write a real observation into the developer's ledger, and a test suite
// that quietly mutates the thing it is testing is worse than no suite.
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const script = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "observe-module-length.mjs",
);

const run = (env) =>
  spawnSync(process.execPath, [script], {
    encoding: "utf8",
    env: { ...process.env, ...env },
  });

const VALID = "0123456789abcdef0123456789abcdef";

// An unattributed observation is worse than none: the ledger would carry a
// measurement nobody is answerable for.
test("a missing session is refused rather than reported anonymously", () => {
  const result = run({ LODESTAR_SESSION_ID: "" });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /LODESTAR_SESSION_ID/);
});

test("a session id that is not a 128-bit token is refused", () => {
  const result = run({ LODESTAR_SESSION_ID: "not-a-token" });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /128-bit/);
});

// The failure this whole class of bug is made of: a reporter that cannot reach
// its control, says nothing, and exits clean looks exactly like a pass.
test("an unreachable Intent Plane fails loudly instead of reporting nothing", () => {
  const result = run({
    LODESTAR_SESSION_ID: VALID,
    LODESTAR_MCP_BIN: path.join("no", "such", "binary"),
  });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /lodestar-mcp/);
  assert.match(
    result.stderr,
    /Reporting nothing is not the same as reporting a pass/,
  );
});

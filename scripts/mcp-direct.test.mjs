import assert from "node:assert/strict";
import { test } from "node:test";

import { runDirectCalls } from "./mcp-direct.mjs";

test("runDirectCalls resolves the plane's binary once and forwards every call in one batch", () => {
  const seen = {};
  const result = runDirectCalls(
    "lodestar",
    [{ name: "open_session", arguments: { a: 1 } }],
    {
      repoRoot: "/repo",
      resolveServerFn: (root, plane) => {
        seen.root = root;
        seen.plane = plane;
        return "/repo/target/release/lodestar-mcp.exe";
      },
      callToolsFn: (binary, cwd, calls) => {
        seen.binary = binary;
        seen.cwd = cwd;
        seen.calls = calls;
        return [{ ok: true }];
      },
    },
  );
  assert.equal(seen.root, "/repo");
  assert.equal(seen.plane, "lodestar");
  assert.equal(seen.binary, "/repo/target/release/lodestar-mcp.exe");
  assert.deepEqual(seen.calls, [{ name: "open_session", arguments: { a: 1 } }]);
  assert.deepEqual(result, [{ ok: true }]);
});

test("runDirectCalls refuses clearly when no binary is found for the plane", () => {
  assert.throws(
    () =>
      runDirectCalls("mindleak", [], {
        repoRoot: "/repo",
        resolveServerFn: () => null,
      }),
    /no mindleak binary found under \/repo\/target/,
  );
});

test("runDirectCalls defaults repoRoot to the current working directory", () => {
  let seenRoot = null;
  assert.throws(() =>
    runDirectCalls("mindleak", [], {
      resolveServerFn: (root) => {
        seenRoot = root;
        return null;
      },
    }),
  );
  assert.equal(seenRoot, process.cwd());
});

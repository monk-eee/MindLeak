// Tests for the stale local server notice. Run with: make script-test
//
// The decision is two timestamps and a path, which is exactly the kind of thing
// that looks obvious and is wrong in one direction. Tested pure, with no
// filesystem and no build.
import { test } from "node:test";
import assert from "node:assert/strict";
import { join } from "node:path";

import { staleServerNotice } from "./claim-gate.mjs";

const REPO = join("C:", "repos", "MindLeak");
const LOCAL = join(REPO, "target", "debug", "lodestar-mcp.exe");

const notice = (over) =>
  staleServerNotice({
    binary: LOCAL,
    repoRoot: REPO,
    binaryMtime: 1_000_000,
    sourceMtime: 900_000,
    ...over,
  });

test("a local build newer than its source is not stale", () => {
  assert.equal(notice(), null);
});

test("a local build older than its source is named, with the rebuild command", () => {
  const message = notice({ binaryMtime: 900_000, sourceMtime: 1_000_000 });
  assert.match(message, /older than the source/);
  assert.match(message, /cargo build -p lodestar-mcp/);
  assert.match(message, /lodestar-mcp\.exe/);
});

// The gap is what makes this expensive: the tool answers, the answer is wrong
// in exactly the way the old code was wrong, and the fix looks broken.
test("the notice says how far behind the build is", () => {
  const message = notice({ binaryMtime: 0, sourceMtime: 42_000 });
  assert.match(message, /42s older/);
});

// An installed release was never built from this tree. Warning that a shipped
// binary is older than crates/ would fire on every run, and a warning that is
// always on is a warning nobody reads.
test("a binary outside the repo's target directory is never judged", () => {
  assert.equal(
    notice({
      binary: join(
        "C:",
        "Users",
        "me",
        ".vscode",
        "extensions",
        "lodestar-mcp.exe",
      ),
      binaryMtime: 0,
      sourceMtime: 1_000_000,
    }),
    null,
  );
});

test("equal timestamps are not stale, because a rebuild lands on the same second", () => {
  assert.equal(notice({ binaryMtime: 500, sourceMtime: 500 }), null);
});

test("a missing binary is nobody's staleness problem", () => {
  assert.equal(notice({ binary: null }), null);
});

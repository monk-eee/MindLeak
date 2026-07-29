// Tests for the build probe. Run with: node scripts/script-tests.mjs
//
// The regression it guards: staleness was diagnosed from file dates and from
// how far a checkout sat behind `main`, and both were wrong. Two worktrees five
// commits behind wrote absolute ids; others far further behind were fine.
import assert from "node:assert/strict";
import { test } from "node:test";

import { binariesUnder, verdictFor } from "./mcp-build-probe.mjs";

test("an absolute id means the binary never made the path repo-relative", () => {
  assert.equal(
    verdictFor(["artifact:C:/Users/dev/Repos/MindLeak/src/a.rs"]),
    "stale",
  );
  // POSIX spelling of the same defect.
  assert.equal(verdictFor(["symbol:/home/dev/repo/src/a.rs:parse"]), "stale");
});

test("a repo-relative id is what a current binary writes", () => {
  assert.equal(verdictFor(["artifact:src/a.rs"]), "current");
  assert.equal(
    verdictFor(["artifact:probe.rs", "symbol:probe.rs:probe"]),
    "current",
  );
});

test("no ids is unknown, never a pass", () => {
  // A binary that refused the call has not been shown to be correct, and
  // reporting it as current would be the reassuring answer rather than the
  // true one.
  assert.equal(verdictFor([]), "unknown");
  assert.equal(verdictFor(undefined), "unknown");
});

test("an id that merely contains a drive-like fragment is not absolute", () => {
  // `artifact:crates/x/C:/y` is not a path this repository produces, but the
  // rule must anchor at the start rather than search anywhere in the id.
  assert.equal(verdictFor(["artifact:crates/x/notes-C:/y.rs"]), "current");
});

test("probing finds release and debug builds across sibling checkouts", () => {
  const present = new Set([
    "/repos/MindLeak/target/release/mindleak-mcp",
    "/repos/MindLeak/target/release/mindleak-mcp.exe",
    "/repos/MindLeak-build/target/debug/mindleak-mcp",
    "/repos/MindLeak-build/target/debug/mindleak-mcp.exe",
  ]);
  const found = binariesUnder(
    "/repos/MindLeak",
    (p) => present.has(p.replace(/\\/g, "/")),
    () => ["MindLeak", "MindLeak-build", "Unrelated"],
  ).map((p) => p.replace(/\\/g, "/"));

  assert.equal(found.length, 2);
  assert.ok(found.some((p) => p.includes("MindLeak/target/release")));
  assert.ok(found.some((p) => p.includes("MindLeak-build/target/debug")));
  // A neighbouring project that is not a checkout of this repository is not ours
  // to probe.
  assert.ok(!found.some((p) => p.includes("Unrelated")));
});

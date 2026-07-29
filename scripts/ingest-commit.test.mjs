// Tests for capturing a commit as evidence. Run with: node scripts/script-tests.mjs
//
// The behaviour that matters is what this refuses to do. A hook on every commit
// is trusted only as long as it is invisible, so the interesting cases are the
// ones where it declines to act.
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  readCommit,
  skippedWarning,
  timeoutMs,
  worthIngesting,
} from "./ingest-commit.mjs";

const fakeGit = (log, changed) => (args) => {
  if (args[0] === "log") return log;
  if (args[0] === "show") return changed;
  throw new Error(`unexpected git ${args[0]}`);
};

test("the commit's own facts are read straight from git", () => {
  const commit = readCommit(
    fakeGit(
      "abc123\u000017854321\u0000fix: something real",
      "src/a.rs\nsrc/b.rs\n",
    ),
  );
  assert.equal(commit.sha, "abc123");
  assert.equal(commit.timestamp, 17854321);
  assert.equal(commit.message, "fix: something real");
  assert.deepEqual(commit.changed, ["src/a.rs", "src/b.rs"]);
});

/// The timestamp is the whole point of passing it. `ingest_commit` defaults to
/// now, and because the node is upserted, a commit recorded at the wrong time
/// stays wrong -- no later window will ever contain it.
test("the commit's timestamp is captured, not the moment of ingestion", () => {
  const commit = readCommit(fakeGit("abc\u00001000000\u0000msg", "a.rs\n"));
  assert.equal(commit.timestamp, 1000000);
  assert.notEqual(commit.timestamp, Math.floor(Date.now() / 1000));
});

/// A merge commit is not new work. Its content already arrived on the branches
/// it joins, so ingesting it would attribute every file in the merge to
/// whoever happened to run it -- and on this repository that is one agent
/// reconciling another's branch.
test("a merge commit is not ingested as new work", () => {
  const commit = { changed: ["a.rs", "b.rs"] };
  assert.equal(worthIngesting(commit, 2), false);
  assert.equal(worthIngesting(commit, 1), true);
});

test("an empty commit has nothing to attribute", () => {
  assert.equal(worthIngesting({ changed: [] }, 1), false);
});

/// Giving up quietly was the original design and it was wrong. A commit landed
/// with no provenance, the evidence bundle came back empty, and the task was
/// uncertifiable -- with nothing connecting that outcome back to a hook that had
/// silently timed out minutes earlier. Never blocking and never reporting are
/// different promises; only the first one is load-bearing.
test("a skipped ingest names the commit and how to backfill it", () => {
  const warning = skippedWarning("abc123", "no response within 5000ms");

  assert.match(warning, /abc123/);
  assert.match(warning, /no response within 5000ms/);
  assert.match(warning, /NOT recorded/);
  // It must say the commit still succeeded, or the committer will think it did not.
  assert.match(warning, /commit succeeded/);
  // And it must say to use the commit's own timestamp, because backfilling with
  // the wrong one is permanent.
  assert.match(warning, /OWN timestamp/);
});

/// The budget stays small by default -- a hook that hangs gets uninstalled --
/// but a loaded machine can spend most of it just starting the server binary,
/// which is exactly how provenance went missing here.
test("the timeout is configurable and defaults to five seconds", () => {
  assert.equal(timeoutMs({}), 5000);
  assert.equal(timeoutMs({ MINDLEAK_INGEST_TIMEOUT_MS: "20000" }), 20000);
});

test("a nonsensical timeout falls back to the default rather than disabling the guard", () => {
  for (const bad of ["", "nonsense", "0", "-1", "NaN"]) {
    assert.equal(timeoutMs({ MINDLEAK_INGEST_TIMEOUT_MS: bad }), 5000, bad);
  }
});

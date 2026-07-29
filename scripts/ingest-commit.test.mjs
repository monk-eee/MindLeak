// Tests for capturing a commit as evidence. Run with: node scripts/script-tests.mjs
//
// The behaviour that matters is what this refuses to do. A hook on every commit
// is trusted only as long as it is invisible, so the interesting cases are the
// ones where it declines to act.
import assert from "node:assert/strict";
import { test } from "node:test";

import { readCommit, worthIngesting } from "./ingest-commit.mjs";

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

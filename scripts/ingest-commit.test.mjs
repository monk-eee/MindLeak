// Tests for capturing a commit as evidence. Run with: node scripts/script-tests.mjs
//
// The behaviour that matters is what this refuses to do. A hook on every commit
// is trusted only as long as it is invisible, so the interesting cases are the
// ones where it declines to act.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
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

/// A clean merge is not new work: its content already arrived on the branches it
/// joins, so ingesting it would attribute every file to whoever happened to run
/// it. Git reports no files for such a merge, because `git show --name-only`
/// lists only what differs from EVERY parent.
test("a clean merge is not ingested as new work", () => {
  assert.equal(worthIngesting({ changed: [] }), false);
});

/// But a conflicted merge IS authored work, and dropping it made reconcile-
/// shaped tasks impossible to certify: the whole product of a reconcile is the
/// merge commit, so the evidence window came back empty every time. Git already
/// isolates the resolutions -- measured across 25 merges in this repository, the
/// files `git show` reports matched "differs from every parent" in 25 of 25.
test("a conflicted merge keeps the provenance of what it resolved", () => {
  assert.equal(worthIngesting({ changed: ["DEVELOPERS.md"] }), true);
});

test("an empty commit has nothing to attribute", () => {
  assert.equal(worthIngesting({ changed: [] }), false);
});

/// The rule above rests entirely on a claim about git: that `git show
/// --name-only` on a merge reports only what differs from EVERY parent. That is
/// load-bearing enough to check against real git rather than a fake, because if
/// it were false a clean merge would attribute another agent's whole branch to
/// whoever ran the merge.
test("git reports only the conflict resolutions for a merge", () => {
  const repo = mkdtempSync(join(tmpdir(), "mindleak-merge-"));
  try {
    const git = (args) =>
      execFileSync("git", args, {
        cwd: repo,
        encoding: "utf8",
        stdio: "pipe",
      }).trim();
    const commit = (message) => git(["commit", "-m", message, "--no-verify"]);

    git(["init", "-b", "main"]);
    git(["config", "user.name", "Merge Test"]);
    git(["config", "user.email", "merge@example.invalid"]);
    writeFileSync(join(repo, "shared.txt"), "base\n");
    writeFileSync(join(repo, "untouched.txt"), "quiet\n");
    git(["add", "."]);
    commit("base");

    // A branch that changes a file nobody else touches.
    git(["checkout", "-b", "theirs"]);
    writeFileSync(join(repo, "untouched.txt"), "their work\n");
    git(["add", "untouched.txt"]);
    commit("theirs");

    // A clean merge: the content arrived from `theirs`, authored by them.
    git(["checkout", "main"]);
    writeFileSync(join(repo, "shared.txt"), "base\nmine\n");
    git(["add", "shared.txt"]);
    commit("mine");
    git(["merge", "theirs", "--no-edit", "-m", "clean merge"]);

    const clean = readCommit(git);
    assert.deepEqual(clean.changed, [], "a clean merge must attribute nothing");
    assert.equal(worthIngesting(clean), false);

    // A conflicting branch, resolved by hand.
    git(["checkout", "-b", "conflicting", "HEAD~2"]);
    writeFileSync(join(repo, "shared.txt"), "base\nconflict\n");
    git(["add", "shared.txt"]);
    commit("conflicting");
    git(["checkout", "main"]);
    try {
      git(["merge", "conflicting", "--no-edit"]);
    } catch {
      // Expected: the merge stops on the conflict.
    }
    writeFileSync(join(repo, "shared.txt"), "base\nresolved by hand\n");
    git(["add", "shared.txt"]);
    commit("conflicted merge");

    const conflicted = readCommit(git);
    assert.deepEqual(
      conflicted.changed,
      ["shared.txt"],
      "a conflicted merge must attribute exactly what was resolved",
    );
    assert.equal(worthIngesting(conflicted), true);
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
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

// Tests for the merged-branch audit. Run with: node scripts/script-tests.mjs
//
// The regression this pins: the audit failed on work that had fully landed.
// It compared ancestry, so a squash or rebase merge — which lands every line
// under a new commit id — looked identical to a branch whose commits were
// never merged at all. It then demanded a follow-up pull request for work
// already on main, which is not a thing anyone can do. An audit with no green
// move available gets switched off, and switching this one off would take the
// check that catches genuinely lost work with it.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { auditBranches, classifyCommits } from "./merge-audit.mjs";

/// Git exports GIT_DIR, GIT_INDEX_FILE and friends to its hooks, and this suite
/// runs from pre-push. Inherited by a test driving git in a temp directory they
/// outrank `cwd`, so git reads the fixture and writes to the REAL repository.
const isolatedGitEnvironment = () => {
  const isolated = { ...process.env };
  for (const variable of [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
  ]) {
    delete isolated[variable];
  }
  return isolated;
};

/// A repository with one branch per way a merge can end, so the classifier is
/// judged against real git rather than a fake. The distinction under test is
/// one only git can draw: whether a patch already exists upstream under some
/// other commit id.
const withFixture = (body) => {
  const repo = mkdtempSync(join(tmpdir(), "mindleak-audit-"));
  try {
    const git = (args) =>
      execFileSync("git", args, {
        cwd: repo,
        encoding: "utf8",
        stdio: "pipe",
        env: isolatedGitEnvironment(),
      }).trim();
    const write = (name, text) => writeFileSync(join(repo, name), text);
    const commit = (message) => git(["commit", "-m", message, "--no-verify"]);

    git(["init", "-b", "main"]);
    git(["config", "user.name", "Audit Test"]);
    git(["config", "user.email", "audit@example.invalid"]);
    write("base.txt", "base\n");
    git(["add", "."]);
    commit("base");

    body({ repo, git, write, commit });
  } finally {
    rmSync(repo, { recursive: true, force: true });
  }
};

test("a squash-merged branch is reported as landed, not as lost work", () => {
  withFixture(({ repo, git, write, commit }) => {
    git(["checkout", "-b", "squashed"]);
    write("feature.txt", "the whole feature\n");
    git(["add", "."]);
    commit("feat: the whole feature");

    // The squash: main gains the same patch under a different commit id, which
    // is precisely what the merge button does and what ancestry cannot see.
    git(["checkout", "main"]);
    write("feature.txt", "the whole feature\n");
    git(["add", "."]);
    commit("feat: the whole feature (#1)");

    const { missing, replaced } = classifyCommits(repo, "main", "squashed");

    assert.deepEqual(missing, [], "the work is on main; nothing is missing");
    assert.equal(replaced.length, 1);
    assert.match(replaced[0], /feat: the whole feature$/);
  });
});

test("a commit that never landed anywhere is still reported as lost", () => {
  // The check has to keep earning its place. Making the squash case pass by
  // reporting nothing would be the easy fix and would delete the whole point.
  withFixture(({ repo, git, write, commit }) => {
    git(["checkout", "-b", "abandoned"]);
    write("orphan.txt", "never merged\n");
    git(["add", "."]);
    commit("feat: work that never landed");

    const { missing, replaced } = classifyCommits(repo, "main", "abandoned");

    assert.equal(missing.length, 1);
    assert.match(missing[0], /work that never landed$/);
    assert.deepEqual(replaced, []);
  });
});

test("a branch merged with a merge commit reports nothing at all", () => {
  withFixture(({ repo, git, write, commit }) => {
    git(["checkout", "-b", "properly-merged"]);
    write("feature.txt", "real work\n");
    git(["add", "."]);
    commit("feat: real work");

    git(["checkout", "main"]);
    git(["merge", "--no-ff", "properly-merged", "-m", "Merge #1"]);

    assert.deepEqual(classifyCommits(repo, "main", "properly-merged"), {
      missing: [],
      replaced: [],
    });
  });
});

test("merging the base into a branch is not work the branch left behind", () => {
  // A merge commit carries no changes of its own, so reporting it as lost work
  // is noise — and it was noise in the report that made the real signal harder
  // to read: the live failure listed a `Merge branch 'main' into ...` commit
  // beside the one genuine finding.
  withFixture(({ repo, git, write, commit }) => {
    git(["checkout", "-b", "long-running"]);
    write("feature.txt", "branch work\n");
    git(["add", "."]);
    commit("feat: branch work");

    git(["checkout", "main"]);
    write("elsewhere.txt", "unrelated main work\n");
    git(["add", "."]);
    commit("chore: unrelated");

    git(["checkout", "long-running"]);
    git(["merge", "main", "--no-edit"]);

    const { missing, replaced } = classifyCommits(repo, "main", "long-running");

    assert.equal(missing.length, 1, "only the branch's own commit is missing");
    assert.match(missing[0], /branch work$/);
    assert.equal(
      [...missing, ...replaced].some((line) => line.startsWith("Merge ")),
      false,
      "a merge commit is neither missing nor replaced",
    );
  });
});

test("a branch the remote no longer has is unverifiable, not lost", () => {
  // A deleted branch cannot be audited either way. Reporting it as lost would
  // fail the build for the ordinary act of tidying up after a merge.
  withFixture(({ repo }) => {
    const [result] = auditBranches(repo, "main", ["origin/deleted-long-ago"]);

    assert.equal(result.verifiable, false);
    assert.deepEqual(result.missing, []);
    assert.deepEqual(result.replaced, []);
  });
});

test("each audited branch is classified independently", () => {
  withFixture(({ repo, git, write, commit }) => {
    git(["checkout", "-b", "landed"]);
    write("landed.txt", "landed\n");
    git(["add", "."]);
    commit("feat: landed");

    git(["checkout", "main"]);
    write("landed.txt", "landed\n");
    git(["add", "."]);
    commit("feat: landed (#2)");

    git(["checkout", "-b", "dropped"]);
    write("dropped.txt", "dropped\n");
    git(["add", "."]);
    commit("feat: dropped");

    const results = auditBranches(repo, "main", ["landed", "dropped"]);

    assert.deepEqual(
      results.map((r) => [r.missing.length, r.replaced.length]),
      [
        [0, 1],
        [1, 0],
      ],
    );
  });
});
